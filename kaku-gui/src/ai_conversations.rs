//! Unified conversation store for Kaku's AI chat overlay.
//!
//! Manages both the active (in-progress) conversation and the archive.
//!
//! Storage layout:
//!   ~/.config/kaku/ai_conversations/
//!     index.json        -- active_id pointer + metadata for all conversations
//!     <id>.json         -- messages for each conversation (active or archived)
//!
//! Cap: at most 100 conversations total (including active). The oldest
//! non-active entry is evicted when adding a new one would exceed the cap.

use anyhow::{Context, Result};
use std::path::PathBuf;

const MAX_CONVERSATIONS: usize = 100;

// ── Public types ──────────────────────────────────────────────────────────────

/// A single persisted chat message (user or assistant turn).
#[derive(Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct PersistedAttachment {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub payload: String,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct PersistedMessage {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub attachments: Vec<PersistedAttachment>,
    /// Sequential index of the user/assistant exchange pair this message belongs to.
    /// 0 for legacy messages (missing field). Used by compaction to avoid splitting pairs.
    #[serde(default)]
    pub round_id: u32,
}

/// Metadata entry in the index for one conversation.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct ConversationMeta {
    pub id: String,
    /// Short summary (≤ 40 chars). May be a placeholder ("…") until async summary arrives.
    pub summary: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub message_count: usize,
}

// ── Internal file shapes ──────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct IndexFile {
    version: u32,
    /// ID of the currently active conversation.
    #[serde(default)]
    active_id: Option<String>,
    #[serde(default)]
    conversations: Vec<ConversationMeta>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ConversationFile {
    version: u32,
    summary: String,
    messages: Vec<PersistedMessage>,
}

// ── Path helpers ──────────────────────────────────────────────────────────────

pub(crate) fn conversations_dir() -> Result<PathBuf> {
    let user_config_path = config::user_config_path();
    let config_dir = user_config_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid user config path"))?;
    Ok(config_dir.join("ai_conversations"))
}

fn index_path(dir: &PathBuf) -> PathBuf {
    dir.join("index.json")
}

fn conversation_path(dir: &PathBuf, id: &str) -> PathBuf {
    dir.join(format!("{}.json", id))
}

// ── ID generation ─────────────────────────────────────────────────────────────

/// Generate a unique conversation ID: `YYYYMMDD-HHMMSS-xxxx` (local time + 4-hex rand).
pub fn generate_id() -> String {
    use chrono::Local;
    let now = Local::now();
    let rand: u16 = fastrand::u16(..);
    format!("{}-{:04x}", now.format("%Y%m%d-%H%M%S"), rand)
}

// ── Index helpers ─────────────────────────────────────────────────────────────

fn load_index_from(dir: &PathBuf) -> IndexFile {
    let path = index_path(dir);
    if !path.exists() {
        return IndexFile::default();
    }
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("Could not read AI conversations index: {e}");
            return IndexFile::default();
        }
    };
    match serde_json::from_str::<IndexFile>(&raw) {
        Ok(f) => f,
        Err(e) => {
            log::warn!("Could not parse AI conversations index: {e}");
            IndexFile::default()
        }
    }
}

fn save_index_to(file: &IndexFile, dir: &PathBuf) -> Result<()> {
    let path = index_path(dir);
    let json = serde_json::to_string_pretty(file).context("serialize index")?;
    write_atomic(&path, &json)
}

fn load_conversation_from(dir: &PathBuf, id: &str) -> Result<Vec<PersistedMessage>> {
    let path = conversation_path(dir, id);
    let raw = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let file: ConversationFile =
        serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    Ok(file.messages)
}

fn write_conversation_to(
    dir: &PathBuf,
    id: &str,
    summary: &str,
    messages: &[PersistedMessage],
) -> Result<()> {
    let path = conversation_path(dir, id);
    let file = ConversationFile {
        version: 1,
        summary: summary.to_string(),
        messages: messages.to_vec(),
    };
    let json = serde_json::to_string_pretty(&file).context("serialize conversation")?;
    write_atomic(&path, &json)
}

/// Remove oldest non-active entries if total count exceeds MAX_CONVERSATIONS.
fn evict_excess(idx: &mut IndexFile, dir: &PathBuf) {
    let active_id = idx.active_id.clone();
    while idx.conversations.len() > MAX_CONVERSATIONS {
        // Find the position of the oldest non-active entry.
        let oldest_pos = idx
            .conversations
            .iter()
            .enumerate()
            .filter(|(_, e)| Some(&e.id) != active_id.as_ref())
            .min_by_key(|(_, e)| e.updated_at)
            .map(|(i, _)| i);
        match oldest_pos {
            Some(pos) => {
                let removed = idx.conversations.remove(pos);
                let _ = std::fs::remove_file(conversation_path(dir, &removed.id));
            }
            None => break, // only the active remains, nothing left to evict
        }
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Load all conversation metadata. Caller may filter out the active_id entry
/// when building the /resume picker list.
pub fn load_index() -> Vec<ConversationMeta> {
    match conversations_dir() {
        Ok(dir) => load_index_from(&dir).conversations,
        Err(_) => vec![],
    }
}

/// Return the current active_id (None if index is missing or unparseable).
#[allow(dead_code)]
pub fn get_active_id() -> Option<String> {
    conversations_dir()
        .ok()
        .and_then(|dir| load_index_from(&dir).active_id)
}

/// Ensure there is a valid active conversation. Creates one if none exists.
/// Returns `(active_id, messages)`.
pub fn ensure_active() -> Result<(String, Vec<PersistedMessage>)> {
    let dir = conversations_dir()?;
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;

    let mut idx = load_index_from(&dir);

    // Try to use the existing active conversation.
    if let Some(ref id) = idx.active_id {
        let path = conversation_path(&dir, id);
        if path.exists() {
            match load_conversation_from(&dir, id) {
                Ok(msgs) => return Ok((id.clone(), msgs)),
                Err(e) => {
                    log::warn!("Active conversation unreadable: {e}; creating a new one")
                }
            }
        }
    }

    // No valid active: create one.
    let id = generate_id();
    write_conversation_to(&dir, &id, "…", &[])?;
    let now = unix_now();
    idx.conversations.insert(
        0,
        ConversationMeta {
            id: id.clone(),
            summary: "…".to_string(),
            created_at: now,
            updated_at: now,
            message_count: 0,
        },
    );
    idx.active_id = Some(id.clone());
    idx.version = 1;
    evict_excess(&mut idx, &dir);
    save_index_to(&idx, &dir)?;
    Ok((id, vec![]))
}

/// Write current messages to `<active_id>.json` and update index stats.
pub fn save_active_messages(active_id: &str, messages: &[PersistedMessage]) -> Result<()> {
    if active_id.is_empty() {
        return Ok(());
    }
    let dir = conversations_dir()?;
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;

    // Preserve any existing summary when overwriting messages.
    let existing_summary = {
        let path = conversation_path(&dir, active_id);
        if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|raw| serde_json::from_str::<ConversationFile>(&raw).ok())
                .map(|f| f.summary)
                .unwrap_or_else(|| "…".to_string())
        } else {
            "…".to_string()
        }
    };
    write_conversation_to(&dir, active_id, &existing_summary, messages)?;

    // Update index stats for this entry.
    let mut idx = load_index_from(&dir);
    let now = unix_now();
    if let Some(entry) = idx.conversations.iter_mut().find(|e| e.id == active_id) {
        entry.updated_at = now;
        entry.message_count = messages.len();
    }
    idx.conversations
        .sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    save_index_to(&idx, &dir)
}

/// Archive the current active and start a new active conversation.
/// The old active stays in the index (its summary is updated asynchronously).
/// Returns the new `active_id`.
pub fn start_new_active() -> Result<String> {
    let dir = conversations_dir()?;
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;

    let new_id = generate_id();
    write_conversation_to(&dir, &new_id, "…", &[])?;

    let mut idx = load_index_from(&dir);
    let now = unix_now();
    idx.conversations.insert(
        0,
        ConversationMeta {
            id: new_id.clone(),
            summary: "…".to_string(),
            created_at: now,
            updated_at: now,
            message_count: 0,
        },
    );
    idx.active_id = Some(new_id.clone());
    idx.version = 1;
    evict_excess(&mut idx, &dir);
    save_index_to(&idx, &dir)?;
    Ok(new_id)
}

/// Switch the active conversation to `target_id` (for /resume).
/// Returns the messages of the target conversation.
pub fn switch_active(target_id: &str) -> Result<Vec<PersistedMessage>> {
    let dir = conversations_dir()?;
    let messages = load_conversation_from(&dir, target_id)
        .with_context(|| format!("load conversation {}", target_id))?;
    let mut idx = load_index_from(&dir);
    idx.active_id = Some(target_id.to_string());
    save_index_to(&idx, &dir)?;
    Ok(messages)
}

/// Update the summary for a conversation (called from the async summary thread).
pub fn update_summary(id: &str, summary: &str) -> Result<()> {
    let dir = conversations_dir()?;
    let mut idx = load_index_from(&dir);
    let mut changed = false;
    for meta in idx.conversations.iter_mut() {
        if meta.id == id {
            meta.summary = summary.to_string();
            changed = true;
            break;
        }
    }
    if changed {
        save_index_to(&idx, &dir)?;
        let conv_path = conversation_path(&dir, id);
        if conv_path.exists() {
            if let Ok(raw) = std::fs::read_to_string(&conv_path) {
                if let Ok(mut file) = serde_json::from_str::<ConversationFile>(&raw) {
                    file.summary = summary.to_string();
                    if let Ok(json) = serde_json::to_string_pretty(&file) {
                        let _ = write_atomic(&conv_path, &json);
                    }
                }
            }
        }
    }
    Ok(())
}

// ── cwd index (used by the `k` CLI) ──────────────────────────────────────────

/// Set or update the `cwd -> conv_id` mapping.
pub fn write_cwd_index(cwd: &str, conv_id: &str) -> Result<()> {
    let dir = conversations_dir()?;
    let path = dir.join("cwd_index.json");
    let mut map: std::collections::HashMap<String, String> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    map.insert(cwd.to_string(), conv_id.to_string());
    std::fs::create_dir_all(&dir)?;
    let json = serde_json::to_string_pretty(&map)?;
    write_atomic(&path, &json)
}

/// Look up a cwd mapping; if present and the conv file still exists, return
/// that conv_id. Otherwise create a new active conversation, record the
/// mapping, and return the new id.
pub fn resolve_or_create_conv_for_cwd(cwd: &str) -> Result<String> {
    let dir = conversations_dir()?;
    let path = dir.join("cwd_index.json");
    let map: std::collections::HashMap<String, String> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    if let Some(id) = map.get(cwd).cloned() {
        if conversation_path(&dir, &id).exists() {
            return Ok(id);
        }
    }
    let new_id = start_new_active()?;
    write_cwd_index(cwd, &new_id)?;
    Ok(new_id)
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn write_atomic(path: &PathBuf, content: &str) -> Result<()> {
    // Use a per-call unique name so concurrent writes from different threads
    // (e.g. save_active_messages from the UI thread and update_summary from
    // the background summary thread) never clobber each other's temp file.
    let rand: u16 = fastrand::u16(..);
    let stem = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = path.with_file_name(format!(".{}_{:04x}.tmp", stem, rand));
    std::fs::write(&tmp, content).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_round_trip_with_active_id() {
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();

        let file = IndexFile {
            version: 1,
            active_id: Some("test-active".to_string()),
            conversations: vec![ConversationMeta {
                id: "test-active".to_string(),
                summary: "test".to_string(),
                created_at: 1000,
                updated_at: 1001,
                message_count: 4,
            }],
        };
        save_index_to(&file, &dir_path).unwrap();

        let loaded = load_index_from(&dir_path);
        assert_eq!(loaded.active_id.as_deref(), Some("test-active"));
        assert_eq!(loaded.conversations.len(), 1);
        assert_eq!(loaded.conversations[0].summary, "test");
    }

    #[test]
    fn cap_removes_oldest_non_active() {
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();

        let active_id = "active-id".to_string();
        // 100 non-active + 1 active = 101 total, one over cap.
        let mut conversations: Vec<ConversationMeta> = (0..100_usize)
            .map(|i| ConversationMeta {
                id: format!("id-{:03}", i),
                summary: String::new(),
                created_at: i as i64,
                updated_at: i as i64,
                message_count: 0,
            })
            .collect();
        conversations.insert(
            0,
            ConversationMeta {
                id: active_id.clone(),
                summary: String::new(),
                created_at: 9999,
                updated_at: 9999,
                message_count: 0,
            },
        );
        assert_eq!(conversations.len(), 101);

        let mut idx = IndexFile {
            version: 1,
            active_id: Some(active_id.clone()),
            conversations,
        };

        // Create a dummy file for "id-000" so remove_file finds it.
        std::fs::write(conversation_path(&dir_path, "id-000"), "{}").unwrap();

        evict_excess(&mut idx, &dir_path);

        assert_eq!(idx.conversations.len(), MAX_CONVERSATIONS);
        assert!(idx.conversations.iter().any(|e| e.id == active_id));
        // id-000 had updated_at=0, should be evicted.
        assert!(!idx.conversations.iter().any(|e| e.id == "id-000"));
        // id-001 stays.
        assert!(idx.conversations.iter().any(|e| e.id == "id-001"));
    }

    #[test]
    fn evict_does_not_remove_active() {
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();

        // Only one conversation and it is the active one.
        let active_id = "only-one".to_string();
        let mut idx = IndexFile {
            version: 1,
            active_id: Some(active_id.clone()),
            conversations: vec![ConversationMeta {
                id: active_id.clone(),
                summary: String::new(),
                created_at: 0,
                updated_at: 0,
                message_count: 0,
            }],
        };
        // len=1 which is <= MAX; evict_excess should be a no-op.
        evict_excess(&mut idx, &dir_path);
        assert_eq!(idx.conversations.len(), 1);
        assert_eq!(idx.conversations[0].id, active_id);
    }

    #[test]
    fn persisted_message_defaults_missing_attachments() {
        let raw = r#"{"role":"user","content":"hello"}"#;
        let msg: PersistedMessage = serde_json::from_str(raw).unwrap();
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "hello");
        assert!(msg.attachments.is_empty());
    }

    #[test]
    fn conversation_file_round_trip_preserves_attachments() {
        let file = ConversationFile {
            version: 1,
            summary: "summary".to_string(),
            messages: vec![PersistedMessage {
                role: "user".to_string(),
                content: "question".to_string(),
                attachments: vec![PersistedAttachment {
                    kind: "cwd".to_string(),
                    label: "@cwd".to_string(),
                    payload: "Directory: /tmp".to_string(),
                }],
                round_id: 0,
            }],
        };
        let json = serde_json::to_string(&file).unwrap();
        let parsed: ConversationFile = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.messages.len(), 1);
        assert_eq!(parsed.messages[0].attachments.len(), 1);
        assert_eq!(parsed.messages[0].attachments[0].label, "@cwd");
        assert_eq!(parsed.messages[0].attachments[0].payload, "Directory: /tmp");
    }
}
