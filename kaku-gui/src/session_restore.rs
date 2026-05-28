use crate::termwindow::TermWindowNotif;
use crate::{frontend, TermWindow};
use anyhow::{anyhow, Context};
use config::GuiPosition;
use mux::pane::{Pane, PaneId};
use mux::tab::{PaneEntry, PaneNode, SerdeUrl, SplitDirectionAndSize, Tab, TabId};
use mux::window::WindowId as MuxWindowId;
use mux::Mux;
use parking_lot::Mutex;
use promise::spawn::spawn;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use wezterm_term::TerminalSize;
use wezterm_toast_notification::persistent_toast_notification;
use window::WindowOps;

const SNAPSHOT_VERSION: u32 = 3;

// Envelope written on app quit: continue-where-you-left-off.
#[derive(Debug, Serialize, Deserialize)]
struct SavedSession {
    version: u32,
    windows: Vec<SavedWindowSnapshot>,
}

// Envelope written when the user closes a single window: undo-close-window.
#[derive(Debug, Serialize, Deserialize)]
struct SavedClosedWindow {
    version: u32,
    window: SavedWindowSnapshot,
}

#[derive(Debug, Serialize, Deserialize)]
struct SavedWindowSnapshot {
    active_tab_idx: usize,
    window_title: String,
    #[serde(default)]
    is_focused: bool,
    tabs: Vec<SavedTabSnapshot>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SavedTabSnapshot {
    title: String,
    pane_tree: SavedPaneNode,
}

#[derive(Debug, Serialize, Deserialize)]
enum SavedPaneNode {
    Empty,
    Split {
        left: Box<SavedPaneNode>,
        right: Box<SavedPaneNode>,
        node: SplitDirectionAndSize,
    },
    Leaf(SavedPaneEntry),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct SavedPaneEntry {
    window_id: MuxWindowId,
    tab_id: TabId,
    pane_id: PaneId,
    title: String,
    size: TerminalSize,
    working_dir: Option<SerdeUrl>,
    domain_name: String,
    is_active_pane: bool,
    is_zoomed_pane: bool,
    workspace: String,
}

impl SavedPaneNode {
    fn from_live(node: PaneNode, mux: &Mux) -> anyhow::Result<Self> {
        match node {
            PaneNode::Empty => Ok(Self::Empty),
            PaneNode::Split { left, right, node } => Ok(Self::Split {
                left: Box::new(Self::from_live(*left, mux)?),
                right: Box::new(Self::from_live(*right, mux)?),
                node,
            }),
            PaneNode::Leaf(entry) => Ok(Self::Leaf(SavedPaneEntry::from_live(entry, mux)?)),
        }
    }

    fn root_size(&self) -> Option<TerminalSize> {
        match self {
            Self::Empty => None,
            Self::Split { node, .. } => Some(node.size()),
            Self::Leaf(entry) => Some(entry.size),
        }
    }

    fn into_pane_node(self) -> PaneNode {
        match self {
            Self::Empty => PaneNode::Empty,
            Self::Split { left, right, node } => PaneNode::Split {
                left: Box::new(left.into_pane_node()),
                right: Box::new(right.into_pane_node()),
                node,
            },
            Self::Leaf(entry) => PaneNode::Leaf(entry.into_pane_entry()),
        }
    }
}

impl SavedPaneEntry {
    fn from_live(entry: PaneEntry, mux: &Mux) -> anyhow::Result<Self> {
        let pane = mux
            .get_pane(entry.pane_id)
            .ok_or_else(|| anyhow!("pane {} not found while building snapshot", entry.pane_id))?;
        let domain = mux.get_domain(pane.domain_id()).ok_or_else(|| {
            anyhow!(
                "domain {} not found while building snapshot for pane {}",
                pane.domain_id(),
                entry.pane_id
            )
        })?;

        Ok(Self {
            window_id: entry.window_id,
            tab_id: entry.tab_id,
            pane_id: entry.pane_id,
            title: entry.title,
            size: entry.size,
            working_dir: entry.working_dir,
            domain_name: domain.domain_name().to_string(),
            is_active_pane: entry.is_active_pane,
            is_zoomed_pane: entry.is_zoomed_pane,
            workspace: entry.workspace,
        })
    }

    fn into_pane_entry(self) -> PaneEntry {
        PaneEntry {
            window_id: self.window_id,
            tab_id: self.tab_id,
            pane_id: self.pane_id,
            title: self.title,
            size: self.size,
            working_dir: self.working_dir,
            is_active_pane: self.is_active_pane,
            is_zoomed_pane: self.is_zoomed_pane,
            workspace: self.workspace,
            cursor_pos: Default::default(),
            physical_top: 0,
            top_row: 0,
            left_col: 0,
            tty_name: None,
        }
    }
}

fn config_dir_file(name: &str) -> PathBuf {
    config::CONFIG_DIRS
        .first()
        .cloned()
        .unwrap_or_else(|| config::HOME_DIR.join(".config").join("kaku"))
        .join(name)
}

fn session_file() -> PathBuf {
    config_dir_file("last_session.json")
}

fn closed_window_file() -> PathBuf {
    config_dir_file("last_closed_window.json")
}

fn collect_leaf_entries(node: &SavedPaneNode, out: &mut Vec<SavedPaneEntry>) {
    match node {
        SavedPaneNode::Empty => {}
        SavedPaneNode::Split { left, right, .. } => {
            collect_leaf_entries(left, out);
            collect_leaf_entries(right, out);
        }
        SavedPaneNode::Leaf(entry) => out.push(entry.clone()),
    }
}

fn cwd_from_working_dir(working_dir: Option<&SerdeUrl>) -> Option<String> {
    let url = working_dir?;
    if url.url.scheme() != "file" {
        return None;
    }
    url.url
        .to_file_path()
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
}

fn focused_window_id() -> Option<MuxWindowId> {
    frontend::try_front_end()
        .and_then(|fe| fe.focused_mux_window_id())
        .or_else(|| {
            let mux = Mux::get();
            let mut windows = mux.iter_windows();
            windows.sort();
            windows.pop()
        })
}

// ---------- Pristine state + logically-closed window tracking ----------

// Counts active RestoringGuards so concurrent / nested restores compose
// correctly. mark_dirty is a no-op when depth > 0, and only the last guard's
// drop clears MUX_DIRTY.
static RESTORING_DEPTH: AtomicUsize = AtomicUsize::new(0);
static MUX_DIRTY: AtomicBool = AtomicBool::new(false);

fn logically_closed() -> &'static Mutex<HashSet<MuxWindowId>> {
    static SET: std::sync::OnceLock<Mutex<HashSet<MuxWindowId>>> = std::sync::OnceLock::new();
    SET.get_or_init(|| Mutex::new(HashSet::new()))
}

pub fn mark_dirty() {
    if RESTORING_DEPTH.load(Ordering::Acquire) > 0 {
        return;
    }
    MUX_DIRTY.store(true, Ordering::Release);
}

fn is_dirty() -> bool {
    MUX_DIRTY.load(Ordering::Acquire)
}

pub fn mark_window_logically_closed(window_id: MuxWindowId) {
    logically_closed().lock().insert(window_id);
}

pub fn forget_logically_closed(window_id: MuxWindowId) {
    logically_closed().lock().remove(&window_id);
}

fn is_window_logically_closed(window_id: MuxWindowId) -> bool {
    logically_closed().lock().contains(&window_id)
}

struct RestoringGuard;

impl RestoringGuard {
    fn new() -> Self {
        RESTORING_DEPTH.fetch_add(1, Ordering::AcqRel);
        Self
    }
}

impl Drop for RestoringGuard {
    fn drop(&mut self) {
        // Only the outermost guard clears MUX_DIRTY: nested / concurrent
        // restores share the gate, and clearing on every drop would let one
        // restore steal the pristine bit from another that is still running.
        let prev = RESTORING_DEPTH.fetch_sub(1, Ordering::AcqRel);
        if prev == 1 {
            MUX_DIRTY.store(false, Ordering::Release);
        }
    }
}

// ---------- Triviality / emptiness ----------

fn is_window_trivial(window_id: MuxWindowId) -> bool {
    let mux = Mux::get();
    let Some(window) = mux.get_window(window_id) else {
        return true;
    };
    if window.len() != 1 {
        return false;
    }
    let Some(tab) = window.get_by_idx(0) else {
        return true;
    };
    let panes = tab.iter_panes_ignoring_zoom();
    if panes.len() != 1 {
        return false;
    }
    let dims = panes[0].pane.get_dimensions();
    // RenderableDimensions::scrollback_rows is the total line count *including*
    // the viewport, so `<= viewport_rows` means no history has scrolled off —
    // i.e. the shell has emitted only its prompt.
    dims.scrollback_rows <= dims.viewport_rows
}

fn is_window_empty(window_id: MuxWindowId) -> bool {
    // For the menu-restore "replace current empty window" check we use the
    // same definition as triviality: 1 tab + 1 pane + no scrollback.
    is_window_trivial(window_id)
}

// ---------- Snapshot building ----------

fn build_snapshot_for_window(window_id: MuxWindowId) -> anyhow::Result<SavedWindowSnapshot> {
    let mux = Mux::get();
    let window = mux
        .get_window(window_id)
        .ok_or_else(|| anyhow!("window {window_id} not found"))?;

    let tabs = window
        .iter()
        .map(|tab| {
            Ok(SavedTabSnapshot {
                title: tab.get_title(),
                pane_tree: SavedPaneNode::from_live(tab.codec_pane_tree(), &mux)?,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    Ok(SavedWindowSnapshot {
        active_tab_idx: window.get_active_idx(),
        window_title: window.get_title().to_string(),
        is_focused: focused_window_id() == Some(window_id),
        tabs,
    })
}

// ---------- Atomic write helpers ----------

fn write_json_atomic<T: Serialize>(file_name: &std::path::Path, value: &T) -> anyhow::Result<()> {
    if let Some(parent) = file_name.parent() {
        config::create_user_owned_dirs(parent)
            .with_context(|| format!("create snapshot dir {}", parent.display()))?;
    }

    let encoded = serde_json::to_string_pretty(value).context("encode snapshot")?;
    // Atomic write: a crash mid-write would otherwise leave a truncated JSON
    // file that fails to parse on the next launch. Write to a sibling temp
    // file and rename on top, which is atomic on POSIX.
    let tmp = file_name.with_file_name(format!(
        "{}.{}.{}.tmp",
        file_name.file_stem().unwrap_or_default().to_string_lossy(),
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::write(&tmp, format!("{encoded}\n"))
        .with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, &file_name)
        .with_context(|| format!("rename {} -> {}", tmp.display(), file_name.display()))?;
    Ok(())
}

// ---------- Save entry points ----------

pub fn save_closed_window_snapshot(window_id: MuxWindowId) -> anyhow::Result<()> {
    if is_window_trivial(window_id) {
        return Ok(());
    }
    let snapshot = build_snapshot_for_window(window_id)?;
    let envelope = SavedClosedWindow {
        version: SNAPSHOT_VERSION,
        window: snapshot,
    };
    write_json_atomic(&closed_window_file(), &envelope)
}

pub fn save_session_snapshot() -> anyhow::Result<()> {
    if !is_dirty() {
        return Ok(());
    }

    let mux = Mux::get();
    let mut window_ids = mux.iter_windows();
    window_ids.sort();

    let mut windows = Vec::new();
    for id in window_ids {
        if is_window_logically_closed(id) {
            continue;
        }
        match build_snapshot_for_window(id) {
            Ok(snap) => windows.push(snap),
            Err(err) => log::debug!("skip window {id} for session snapshot: {err:#}"),
        }
    }

    // Don't overwrite a useful saved session with a session that is entirely
    // trivial (e.g. a single fresh shell prompt the user opened and closed).
    if windows.is_empty() {
        return Ok(());
    }
    let all_trivial = windows.iter().all(|w| {
        w.tabs.len() <= 1
            && matches!(
                &w.tabs.first().map(|t| &t.pane_tree),
                Some(SavedPaneNode::Leaf(_)) | None
            )
    });
    if windows.len() == 1 && all_trivial {
        // Single window with one tab containing one leaf pane and no extra
        // structure — likely the startup empty window. Skip.
        let mut leaves = Vec::new();
        if let Some(t) = windows[0].tabs.first() {
            collect_leaf_entries(&t.pane_tree, &mut leaves);
        }
        if leaves.len() <= 1 {
            return Ok(());
        }
    }

    let session = SavedSession {
        version: SNAPSHOT_VERSION,
        windows,
    };
    write_json_atomic(&session_file(), &session)
}

// ---------- Load entry points ----------

fn load_session_from_path(file_name: &std::path::Path) -> anyhow::Result<Option<SavedSession>> {
    let contents = match std::fs::read_to_string(file_name) {
        Ok(s) => s,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(anyhow::Error::new(err).context(format!("read {}", file_name.display())));
        }
    };

    let session: SavedSession = match serde_json::from_str(&contents) {
        Ok(s) => s,
        Err(err) => {
            log::warn!(
                "ignoring corrupt session snapshot at {}: {err}",
                file_name.display()
            );
            return Ok(None);
        }
    };

    if session.version != SNAPSHOT_VERSION {
        log::warn!(
            "ignoring session snapshot at {} with unsupported version {} (expected {})",
            file_name.display(),
            session.version,
            SNAPSHOT_VERSION
        );
        return Ok(None);
    }

    if session.windows.is_empty() {
        return Ok(None);
    }

    Ok(Some(session))
}

fn load_session() -> anyhow::Result<Option<SavedSession>> {
    load_session_from_path(&session_file())
}

fn load_closed_window_from_path(
    file_name: &std::path::Path,
) -> anyhow::Result<Option<SavedClosedWindow>> {
    let contents = match std::fs::read_to_string(file_name) {
        Ok(s) => s,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(anyhow::Error::new(err).context(format!("read {}", file_name.display())));
        }
    };

    let closed: SavedClosedWindow = match serde_json::from_str(&contents) {
        Ok(c) => c,
        Err(err) => {
            log::warn!(
                "ignoring corrupt closed-window snapshot at {}: {err}",
                file_name.display()
            );
            return Ok(None);
        }
    };

    if closed.version != SNAPSHOT_VERSION {
        log::warn!(
            "ignoring closed-window snapshot at {} with unsupported version {} (expected {})",
            file_name.display(),
            closed.version,
            SNAPSHOT_VERSION
        );
        return Ok(None);
    }

    Ok(Some(closed))
}

fn load_closed_window() -> anyhow::Result<Option<SavedClosedWindow>> {
    load_closed_window_from_path(&closed_window_file())
}

fn delete_closed_window_file() {
    let path = closed_window_file();
    if let Err(err) = std::fs::remove_file(&path) {
        if err.kind() != std::io::ErrorKind::NotFound {
            log::debug!("could not remove {}: {err:#}", path.display());
        }
    }
}

// ---------- Restore ----------

async fn spawn_panes_for_tab(
    root: &SavedPaneNode,
) -> anyhow::Result<std::collections::HashMap<PaneId, Arc<dyn Pane>>> {
    let mux = Mux::get();
    let encoding = config::configuration().default_encoding;
    let mut entries = Vec::new();
    collect_leaf_entries(root, &mut entries);

    let mut panes = std::collections::HashMap::new();
    for entry in entries {
        let domain = mux
            .get_domain_by_name(&entry.domain_name)
            .ok_or_else(|| anyhow!("snapshot domain `{}` is not available", entry.domain_name))?;
        let pane = domain
            .spawn_pane(
                &mux,
                entry.size,
                None,
                cwd_from_working_dir(entry.working_dir.as_ref()),
                encoding,
            )
            .await
            .with_context(|| {
                format!(
                    "spawn pane for snapshot pane {} in domain `{}`",
                    entry.pane_id, entry.domain_name
                )
            })?;
        panes.insert(entry.pane_id, pane);
    }

    Ok(panes)
}

async fn get_existing_terminal_size() -> Option<TerminalSize> {
    let window = frontend::try_front_end()?
        .gui_windows()
        .into_iter()
        .next()
        .map(|w| w.window.clone())?;
    let (tx, rx) = smol::channel::bounded::<TerminalSize>(1);
    window.notify(TermWindowNotif::Apply(Box::new(
        move |tw: &mut TermWindow| {
            let _ = tx.try_send(tw.get_terminal_size());
        },
    )));
    rx.recv().await.ok()
}

async fn build_tabs_into_window(
    window_id: MuxWindowId,
    tabs: Vec<SavedTabSnapshot>,
    actual_size: Option<TerminalSize>,
) -> anyhow::Result<()> {
    let mux = Mux::get();
    for saved_tab in tabs {
        let size = saved_tab.pane_tree.root_size().unwrap_or_default();
        let tab = Arc::new(Tab::new(&size));
        let panes = spawn_panes_for_tab(&saved_tab.pane_tree).await?;
        let pane_tree = saved_tab.pane_tree.into_pane_node();

        tab.try_sync_with_pane_tree(size, pane_tree, |entry| {
            panes
                .get(&entry.pane_id)
                .cloned()
                .ok_or_else(|| anyhow!("missing restored pane {}", entry.pane_id))
        })?;

        if !saved_tab.title.is_empty() {
            tab.set_title(&saved_tab.title);
        }

        mux.add_tab_no_panes(&tab);
        mux.add_tab_to_window(&tab, window_id)?;

        if let Some(s) = actual_size {
            tab.resize(s);
        }
    }
    Ok(())
}

async fn restore_window(
    snapshot: SavedWindowSnapshot,
    current_window_id: Option<MuxWindowId>,
) -> anyhow::Result<MuxWindowId> {
    let mux = Mux::get();
    let SavedWindowSnapshot {
        active_tab_idx,
        window_title,
        is_focused: _,
        tabs,
    } = snapshot;
    let actual_size = get_existing_terminal_size().await;

    // The user pressed the menu inside `current_window_id`; that window is
    // clearly live again even if it was previously closed and re-shown via
    // the macOS Dock. Drop any stale "logically closed" marker before we
    // decide replace-vs-newwindow so the next save sees consistent state.
    if let Some(window_id) = current_window_id {
        forget_logically_closed(window_id);
        if is_window_empty(window_id) {
            let existing_tab_ids: Vec<TabId> = match mux.get_window(window_id) {
                Some(window) => window.iter().map(|t| t.tab_id()).collect(),
                None => Vec::new(),
            };

            // Spawn new tabs first to avoid a tab-less window flash.
            build_tabs_into_window(window_id, tabs, actual_size).await?;

            // Now drop the originally-empty tabs.
            for old in existing_tab_ids {
                mux.remove_tab(old);
            }

            if let Some(mut window) = mux.get_window_mut(window_id) {
                if !window_title.is_empty() {
                    window.set_title(&window_title);
                }
                if window.len() > 0 {
                    let max_idx = window.len() - 1;
                    window.set_active_without_saving(active_tab_idx.min(max_idx));
                }
            }

            return Ok(window_id);
        }
    }

    // Otherwise create a new mux window.
    let workspace = mux.active_workspace();
    let builder = mux.new_empty_window(Some(workspace), None::<GuiPosition>);
    let new_window_id = *builder;

    let result = async {
        build_tabs_into_window(new_window_id, tabs, actual_size).await?;

        if let Some(mut window) = mux.get_window_mut(new_window_id) {
            if !window_title.is_empty() {
                window.set_title(&window_title);
            }
            if window.len() > 0 {
                let max_idx = window.len() - 1;
                window.set_active_without_saving(active_tab_idx.min(max_idx));
            }
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;

    match result {
        Ok(()) => {
            drop(builder);
            Ok(new_window_id)
        }
        Err(err) => {
            builder.cancel();
            Err(err)
        }
    }
}

async fn restore_session(
    session: SavedSession,
    current_window_id: Option<MuxWindowId>,
) -> anyhow::Result<()> {
    let _guard = RestoringGuard::new();

    let SavedSession {
        version: _,
        windows,
    } = session;
    if windows.is_empty() {
        return Ok(());
    }

    let focused_idx = windows.iter().position(|w| w.is_focused).unwrap_or(0);

    let mut new_window_ids: Vec<MuxWindowId> = Vec::with_capacity(windows.len());
    for (idx, window_snap) in windows.into_iter().enumerate() {
        let target = if idx == 0 { current_window_id } else { None };
        match restore_window(window_snap, target).await {
            Ok(id) => new_window_ids.push(id),
            Err(err) => log::warn!("failed to restore one window from session: {err:#}"),
        }
    }

    // Best-effort focus on the previously-focused window. The GUI TermWindow
    // for a freshly-created mux window is spawned asynchronously, so the
    // lookup may miss; that is acceptable — focus then stays on whichever
    // window the platform picked.
    if let Some(&target_id) = new_window_ids.get(focused_idx) {
        if let Some(fe) = frontend::try_front_end() {
            if let Some(gui) = fe.gui_window_for_mux_window(target_id) {
                gui.window.focus();
            }
        }
    }

    Ok(())
}

pub fn restore_previous_window_from_menu(current_window_id: Option<MuxWindowId>) {
    spawn(async move {
        let result = async {
            match load_closed_window()? {
                Some(closed) => {
                    let _guard = RestoringGuard::new();
                    restore_window(closed.window, current_window_id).await?;
                    drop(_guard);
                    // Consume the snapshot only on success: if restore failed
                    // (e.g. domain unavailable), keep the file so the user can
                    // retry after fixing the underlying issue.
                    delete_closed_window_file();
                    Ok::<bool, anyhow::Error>(true)
                }
                None => Ok(false),
            }
        }
        .await;

        match result {
            Ok(true) => {}
            Ok(false) => {
                persistent_toast_notification(
                    "Restore Previous Window",
                    "No previously-closed window is available to restore.",
                );
            }
            Err(err) => {
                log::warn!("failed to restore previous window: {err:#}");
                persistent_toast_notification("Restore Previous Window", &format!("{err:#}"));
            }
        }
    })
    .detach();
}

pub async fn try_restore_on_startup() -> anyhow::Result<bool> {
    match load_session()? {
        Some(session) => {
            // macOS's applicationOpenUntitledFile can dispatch a SpawnWindow
            // before this runs, leaving a pristine empty mux window in place.
            // Reuse it as the target for the first restored window so the user
            // does not end up with one phantom empty window plus the restored
            // ones.
            let preexisting_empty = Mux::get()
                .iter_windows()
                .into_iter()
                .find(|id| is_window_empty(*id));
            restore_session(session, preexisting_empty).await?;
            Ok(true)
        }
        None => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_window(title: &str) -> SavedWindowSnapshot {
        SavedWindowSnapshot {
            active_tab_idx: 0,
            window_title: title.to_string(),
            is_focused: false,
            tabs: vec![SavedTabSnapshot {
                title: "Test Tab".to_string(),
                pane_tree: SavedPaneNode::Empty,
            }],
        }
    }

    fn sample_session(version: u32) -> SavedSession {
        SavedSession {
            version,
            windows: vec![sample_window("Test Window")],
        }
    }

    fn sample_closed(version: u32) -> SavedClosedWindow {
        SavedClosedWindow {
            version,
            window: sample_window("Test Window"),
        }
    }

    #[test]
    fn session_round_trips_via_atomic_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("last_session.json");
        write_json_atomic(&path, &sample_session(SNAPSHOT_VERSION)).unwrap();

        let loaded = load_session_from_path(&path).unwrap().expect("session");
        assert_eq!(loaded.version, SNAPSHOT_VERSION);
        assert_eq!(loaded.windows.len(), 1);
        assert_eq!(loaded.windows[0].window_title, "Test Window");
    }

    #[test]
    fn closed_window_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("last_closed_window.json");
        write_json_atomic(&path, &sample_closed(SNAPSHOT_VERSION)).unwrap();

        let loaded = load_closed_window_from_path(&path)
            .unwrap()
            .expect("closed window");
        assert_eq!(loaded.version, SNAPSHOT_VERSION);
        assert_eq!(loaded.window.window_title, "Test Window");
    }

    #[test]
    fn corrupt_session_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("last_session.json");
        std::fs::write(&path, "{not json").unwrap();
        assert!(load_session_from_path(&path).unwrap().is_none());
    }

    #[test]
    fn unsupported_session_version_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("last_session.json");
        std::fs::write(
            &path,
            serde_json::to_string(&sample_session(SNAPSHOT_VERSION + 1)).unwrap(),
        )
        .unwrap();
        assert!(load_session_from_path(&path).unwrap().is_none());
    }

    #[test]
    fn v2_snapshot_is_ignored() {
        // Pre-v3 single-window envelope: must be silently ignored on upgrade.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("last_session.json");
        std::fs::write(
            &path,
            r#"{"version":2,"active_tab_idx":0,"window_title":"x","tabs":[]}"#,
        )
        .unwrap();
        assert!(load_session_from_path(&path).unwrap().is_none());
    }

    // Shared by every test that touches MUX_DIRTY / RESTORING_DEPTH so they
    // serialize against cargo's parallel test runner.
    static DIRTY_TEST_GATE: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

    #[test]
    fn pristine_state_machine() {
        let _g = DIRTY_TEST_GATE.lock();

        MUX_DIRTY.store(false, Ordering::Release);
        RESTORING_DEPTH.store(0, Ordering::Release);
        assert!(!is_dirty());

        // mark_dirty outside a restore should flip the bit.
        mark_dirty();
        assert!(is_dirty());

        // Inside RestoringGuard, mark_dirty is a no-op (the previously-set
        // bit remains), and dropping the outermost guard forces dirty back
        // to false.
        {
            let _guard = RestoringGuard::new();
            mark_dirty();
            assert!(MUX_DIRTY.load(Ordering::Acquire));
        }
        assert!(!is_dirty());
    }

    #[test]
    fn nested_restoring_guards_compose() {
        let _g = DIRTY_TEST_GATE.lock();

        MUX_DIRTY.store(false, Ordering::Release);
        RESTORING_DEPTH.store(0, Ordering::Release);

        let outer = RestoringGuard::new();
        {
            let _inner = RestoringGuard::new();
            // Both guards are active: depth == 2.
            assert_eq!(RESTORING_DEPTH.load(Ordering::Acquire), 2);
        }
        // Inner dropped; depth == 1 and MUX_DIRTY still untouched.
        assert_eq!(RESTORING_DEPTH.load(Ordering::Acquire), 1);
        // Even if something marks dirty here, it must be ignored — depth > 0.
        mark_dirty();
        assert!(!is_dirty());
        drop(outer);
        // Outer dropped; depth == 0 and dirty cleared by the outer drop.
        assert_eq!(RESTORING_DEPTH.load(Ordering::Acquire), 0);
        assert!(!is_dirty());
    }

    #[test]
    fn logically_closed_set_round_trips() {
        // Use a high id unlikely to collide with concurrent test windows.
        let id: MuxWindowId = 999_999;
        forget_logically_closed(id);
        assert!(!is_window_logically_closed(id));
        mark_window_logically_closed(id);
        assert!(is_window_logically_closed(id));
        forget_logically_closed(id);
        assert!(!is_window_logically_closed(id));
    }
}
