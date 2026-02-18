-- Kaku Configuration

local wezterm = require 'wezterm'

local config = {}

-- `config_builder` validates every assignment and is expensive on large configs.
-- Keep startup fast by default; enable strict validation only when debugging config.
if os.getenv('KAKU_STRICT_CONFIG') == '1' and wezterm.config_builder then
  config = wezterm.config_builder()
end



local function basename(path)
  return path:match('([^/]+)$')
end

-- URL decode helper for Chinese characters in paths
-- Converts %E9%9F%B3%E4%B9%90 -> 音乐
local function url_decode(str)
  if not str then
    return str
  end
  -- First, handle UTF-8 encoded sequences (%XX%YY%ZZ)
  local result = str:gsub('%%([0-9A-Fa-f][0-9A-Fa-f])', function(hex)
    return string.char(tonumber(hex, 16))
  end)
  return result
end

local function padding_matches(current, expected)
  return current
    and current.left == expected.left
    and current.right == expected.right
    and current.top == expected.top
    and current.bottom == expected.bottom
end

local fullscreen_uniform_padding = {
  left = '40px',
  right = '40px',
  top = '70px',
  bottom = '30px',
}

-- Per-window resize debounce state.
-- Weak keys ensure closed windows don't leak state.
local resize_state_by_window = setmetatable({}, { __mode = 'k' })

local function monotonic_now()
  -- Keep this numeric for debounce arithmetic.
  -- Some runtime environments expose os.clock() as a non-number value.
  local ok, value = pcall(os.clock)
  if ok and type(value) == 'number' then
    return value
  end
  return os.time()
end

local function dims_hash(dims)
  return dims.pixel_width .. "x" .. dims.pixel_height
end

local function update_window_config(window, is_full_screen)
  local now = monotonic_now()
  local dims = window:get_dimensions()
  local current_hash = dims_hash(dims)
  local state = resize_state_by_window[window]
  if not state then
    state = {
      last_resize_time = 0,
      last_dims_hash = "",
    }
    resize_state_by_window[window] = state
  end
  local overrides = window:get_config_overrides() or {}
  local needs_update = false

  if is_full_screen then
    needs_update = (not padding_matches(overrides.window_padding, fullscreen_uniform_padding))
      or overrides.hide_tab_bar_if_only_one_tab ~= false
  else
    needs_update = overrides.window_padding ~= nil or overrides.hide_tab_bar_if_only_one_tab ~= nil
  end

  -- Skip update if dimensions changed rapidly (within 1 second) and state is stable
  -- This prevents padding flicker during fullscreen animation
  if current_hash ~= state.last_dims_hash then
    local time_since_last = now - state.last_resize_time
    if time_since_last < 1.0 and not needs_update then
      -- Rapid change detected, skip this update
      state.last_dims_hash = current_hash
      state.last_resize_time = now
      return
    end
    state.last_dims_hash = current_hash
    state.last_resize_time = now
  end

  if is_full_screen then
    if not padding_matches(overrides.window_padding, fullscreen_uniform_padding) or overrides.hide_tab_bar_if_only_one_tab ~= false then
      overrides.window_padding = fullscreen_uniform_padding
      overrides.hide_tab_bar_if_only_one_tab = false
      window:set_config_overrides(overrides)
    end
    return
  end

  if overrides.window_padding ~= nil or overrides.hide_tab_bar_if_only_one_tab ~= nil then
    overrides.window_padding = nil
    overrides.hide_tab_bar_if_only_one_tab = nil
    window:set_config_overrides(overrides)
  end
end

local function extract_path_from_cwd(cwd)
  if not cwd then
    return ''
  end

  local path = ''
  if type(cwd) == 'table' then
    path = cwd.file_path or cwd.path or tostring(cwd)
  else
    path = tostring(cwd)
  end

  path = path:gsub('^file://[^/]*', ''):gsub('/$', '')
  -- Decode URL-encoded characters (e.g., %E9%9F%B3%E4%B9%90 -> 音乐)
  path = url_decode(path)
  return path
end

local active_tab_cwd_cache = {}
-- os.time() returns integer wall-clock seconds; 1s granularity is fine for tab title throttle
local active_tab_cwd_refresh_interval = 1
local function now_secs()
  return os.time()
end

local home_dir = os.getenv("HOME")
local kaku_state_dir = home_dir and (home_dir .. "/.config/kaku") or nil
local lazygit_state_file = kaku_state_dir and (kaku_state_dir .. "/lazygit_state.json") or nil
local lazygit_state_cache = nil
local lazygit_repo_probe_cache = {}
local lazygit_repo_probe_interval_secs = 5
local lazygit_command_probe = { value = nil, command = nil, checked_at = 0 }
local lazygit_command_probe_interval_secs = 30

local function trim_trailing_whitespace(value)
  if type(value) ~= "string" then
    return ""
  end
  return value:gsub("%s+$", "")
end

local function ensure_kaku_state_dir()
  if not kaku_state_dir or kaku_state_dir == "" then
    return
  end
  os.execute(string.format("mkdir -p %q", kaku_state_dir))
end

local function load_lazygit_state()
  if lazygit_state_cache then
    return lazygit_state_cache
  end

  local state = { repos = {} }
  if not lazygit_state_file then
    lazygit_state_cache = state
    return state
  end

  local file = io.open(lazygit_state_file, "r")
  if file then
    local raw = file:read("*all")
    file:close()
    if raw and raw ~= "" then
      local ok, parsed = pcall(wezterm.json_parse, raw)
      if ok and type(parsed) == "table" then
        state = parsed
      end
    end
  end

  if type(state.repos) ~= "table" then
    state.repos = {}
  end

  lazygit_state_cache = state
  return state
end

local function save_lazygit_state()
  if not lazygit_state_file then
    return
  end

  ensure_kaku_state_dir()
  local state = load_lazygit_state()
  local ok, encoded = pcall(wezterm.json_encode, state)
  if not ok or type(encoded) ~= "string" or encoded == "" then
    return
  end

  local file = io.open(lazygit_state_file, "w")
  if not file then
    return
  end
  file:write(encoded .. "\n")
  file:close()
end

local function get_lazygit_repo_flags(repo_root)
  local repos = load_lazygit_state().repos
  local flags = repos[repo_root]
  if type(flags) ~= "table" then
    flags = { hinted = false, used = false }
    repos[repo_root] = flags
  end
  flags.hinted = flags.hinted == true
  flags.used = flags.used == true
  return flags
end

local function mark_repo_lazygit_hinted(repo_root)
  if not repo_root or repo_root == "" then
    return
  end
  local flags = get_lazygit_repo_flags(repo_root)
  if flags.hinted then
    return
  end
  flags.hinted = true
  save_lazygit_state()
end

local function mark_repo_lazygit_used(repo_root)
  if not repo_root or repo_root == "" then
    return
  end
  local flags = get_lazygit_repo_flags(repo_root)
  if flags.used then
    return
  end
  flags.used = true
  save_lazygit_state()
end

local function pane_cwd(pane)
  if not pane then
    return ""
  end

  local ok, runtime_cwd = pcall(function()
    return pane:get_current_working_dir()
  end)
  if ok and runtime_cwd then
    local path = extract_path_from_cwd(runtime_cwd)
    if path ~= "" then
      return path
    end
  end

  return extract_path_from_cwd(pane.current_working_dir)
end

local function detect_git_repo_root(path)
  if not path or path == "" then
    return nil
  end

  local ok, stdout = wezterm.run_child_process({
    "git",
    "-C",
    path,
    "rev-parse",
    "--show-toplevel",
  })
  if not ok then
    return nil
  end

  local repo_root = trim_trailing_whitespace(stdout)
  if repo_root == "" then
    return nil
  end
  return repo_root
end

local function repo_has_pending_changes(repo_root)
  local ok, stdout = wezterm.run_child_process({
    "git",
    "-C",
    repo_root,
    "status",
    "--porcelain",
    "--untracked-files=normal",
  })
  if not ok then
    return false
  end
  return trim_trailing_whitespace(stdout) ~= ""
end

local function git_repo_context(path)
  local now = now_secs()
  local cached = lazygit_repo_probe_cache[path]
  if cached and (now - cached.checked_at) < lazygit_repo_probe_interval_secs then
    return cached.repo_root, cached.has_changes
  end

  local repo_root = detect_git_repo_root(path)
  local has_changes = false
  if repo_root then
    has_changes = repo_has_pending_changes(repo_root)
  end

  lazygit_repo_probe_cache[path] = {
    checked_at = now,
    repo_root = repo_root,
    has_changes = has_changes,
  }

  return repo_root, has_changes
end

local function resolve_lazygit_command()
  local now = now_secs()
  local cached_value = lazygit_command_probe.value
  if cached_value ~= nil then
    local age = now - lazygit_command_probe.checked_at
    if cached_value or age < lazygit_command_probe_interval_secs then
      return lazygit_command_probe.command
    end
  end

  local candidates = {
    "lazygit",
    "/opt/homebrew/bin/lazygit",
    "/usr/local/bin/lazygit",
  }
  local resolved = nil
  for _, cmd in ipairs(candidates) do
    local call_ok, run_result = pcall(function()
      return select(1, wezterm.run_child_process({ cmd, "--version" }))
    end)
    if call_ok and run_result then
      resolved = cmd
      break
    end
  end

  lazygit_command_probe.value = resolved ~= nil
  lazygit_command_probe.command = resolved
  lazygit_command_probe.checked_at = now
  return resolved
end

local function is_lazygit_installed()
  return resolve_lazygit_command() ~= nil
end

local function pane_is_lazygit(pane)
  if not pane then
    return false
  end

  local ok, proc = pcall(function()
    return pane:get_foreground_process_name()
  end)
  if not ok or type(proc) ~= "string" or proc == "" then
    return false
  end

  return basename(proc) == "lazygit"
end

local function resolve_active_pane(window, pane)
  if pane then
    return pane
  end
  if not window then
    return nil
  end

  local ok_tab, tab = pcall(function()
    return window:active_tab()
  end)
  if ok_tab and tab then
    local ok_pane, active_pane = pcall(function()
      return tab:active_pane()
    end)
    if ok_pane then
      return active_pane
    end
  end

  return nil
end

local function show_lazygit_toast(window, pane, event_name)
  if not window then
    return
  end
  pcall(function()
    window:perform_action(wezterm.action.EmitEvent(event_name), pane)
  end)
end

local function maybe_show_lazygit_hint(window, pane)
  pane = resolve_active_pane(window, pane)

  local path = pane_cwd(pane)
  if path == "" then
    return
  end

  local repo_root, has_changes = git_repo_context(path)
  if not repo_root then
    return
  end

  if pane_is_lazygit(pane) then
    mark_repo_lazygit_used(repo_root)
    return
  end

  local flags = get_lazygit_repo_flags(repo_root)
  if flags.hinted or flags.used or not has_changes then
    return
  end

  if not is_lazygit_installed() then
    return
  end

  show_lazygit_toast(window, pane, "kaku-toast-try-lazygit")
  mark_repo_lazygit_hinted(repo_root)
end

local function launch_lazygit(window, pane)
  pane = resolve_active_pane(window, pane)
  if not pane then
    show_lazygit_toast(window, pane, "kaku-toast-lazygit-no-pane")
    return
  end

  local path = pane_cwd(pane)
  if path == "" then
    show_lazygit_toast(window, pane, "kaku-toast-lazygit-no-cwd")
    return
  end

  local repo_root = detect_git_repo_root(path)
  if not repo_root then
    show_lazygit_toast(window, pane, "kaku-toast-lazygit-not-git")
    return
  end

  local lazygit_cmd = resolve_lazygit_command()
  if not lazygit_cmd then
    show_lazygit_toast(window, pane, "kaku-toast-lazygit-missing")
    return
  end

  local ok = pcall(function()
    window:perform_action(
      wezterm.action.SendString(lazygit_cmd .. "\r"),
      pane
    )
  end)
  if not ok then
    show_lazygit_toast(window, pane, "kaku-toast-lazygit-dispatch-failed")
    return
  end
  mark_repo_lazygit_used(repo_root)
end

local function evict_stale_cache(live_pane_ids)
  for pane_id in pairs(active_tab_cwd_cache) do
    if not live_pane_ids[pane_id] then
      active_tab_cwd_cache[pane_id] = nil
    end
  end
end

local function tab_path_parts(tab)
  local pane = tab.active_pane
  if not pane then
    return '', ''
  end

  local source_cwd = pane.current_working_dir
  local source_path = extract_path_from_cwd(source_cwd)
  local path = source_path

  if tab.is_active then
    local pane_id = tostring(pane.pane_id)
    local now = now_secs()
    local cached = active_tab_cwd_cache[pane_id]
    local should_refresh = (not cached)
      or path == ''
      or source_path ~= cached.source_path
      or (now - cached.updated_at) >= active_tab_cwd_refresh_interval

    if should_refresh then
      local ok, runtime_cwd = pcall(function()
        return pane:get_current_working_dir()
      end)
      if ok and runtime_cwd then
        local runtime_path = extract_path_from_cwd(runtime_cwd)
        if runtime_path ~= '' then
          path = runtime_path
        end
      end

      active_tab_cwd_cache[pane_id] = {
        path = path,
        source_path = source_path,
        updated_at = now,
      }
    elseif cached and cached.path ~= '' then
      path = cached.path
    end
  elseif path == '' then
    local ok, runtime_cwd = pcall(function()
      return pane:get_current_working_dir()
    end)
    if ok and runtime_cwd then
      path = extract_path_from_cwd(runtime_cwd)
    end
  end

  if path == '' then
    return '', ''
  end

  local current = basename(path) or path
  local parent_path = path:match('(.+)/[^/]+$') or ''
  local parent = basename(parent_path) or parent_path
  return parent, current
end

wezterm.on('format-tab-title', function(tab, tabs, _, effective_config, hover, max_width)
  -- Evict stale cache only on the first tab to avoid O(n²) across the render cycle
  if tab.tab_index == 0 then
    local live_pane_ids = {}
    for _, t in ipairs(tabs) do
      if t.active_pane then
        live_pane_ids[tostring(t.active_pane.pane_id)] = true
      end
    end
    evict_stale_cache(live_pane_ids)
  end

  local parent, current = tab_path_parts(tab)
  local text = current
  if parent ~= '' and current ~= '' then
    text = parent .. '/' .. current
  end

  -- Guard active_pane nil before accessing .title / .is_zoomed
  local active_pane = tab.active_pane
  if text == '' and active_pane then
    text = active_pane.title
  end
  if active_pane and active_pane.is_zoomed then
    text = text .. ' [Z]'
  end
  text = wezterm.truncate_right(text, math.max(8, max_width - 2))

  local intensity = tab.is_active and 'Bold' or 'Normal'
  -- resolved_palette.tab_bar and its sub-fields are all optional; guard each level
  local tab_bar_colors = effective_config.resolved_palette.tab_bar
  local fg
  if tab_bar_colors then
    local entry
    if tab.is_active then
      entry = tab_bar_colors.active_tab
    elseif hover then
      entry = tab_bar_colors.inactive_tab_hover or tab_bar_colors.inactive_tab
    else
      entry = tab_bar_colors.inactive_tab
    end
    fg = entry and entry.fg_color
  end
  -- fallback defaults when palette entry or sub-field is absent
  if not fg then
    fg = tab.is_active and '#edecee' or (hover and '#9b9b9b' or '#6b6b6b')
  end
  return {
    { Attribute = { Intensity = intensity } },
    { Foreground = { Color = fg } },
    { Text = ' ' .. text .. ' ' },
  }
end)

wezterm.on('window-resized', function(window, _)
  local dims = window:get_dimensions()
  update_window_config(window, dims.is_full_screen)
end)

wezterm.on('kaku-launch-lazygit', function(window, pane)
  launch_lazygit(window, pane)
end)

wezterm.on('update-right-status', function(window, pane)
  maybe_show_lazygit_hint(window, pane)

  local dims = window:get_dimensions()
  if not dims.is_full_screen then
    window:set_right_status('')
    return
  end

  local clock_icon = wezterm.nerdfonts.md_clock_time_four_outline
    or wezterm.nerdfonts.md_clock_outline
    or ''
  local text = wezterm.strftime('%H:%M')
  if clock_icon ~= '' then
    window:set_right_status(wezterm.format({
      { Foreground = { Color = '#6b6b6b' } },
      { Text = ' ' .. clock_icon .. ' ' .. text .. ' ' },
    }))
    return
  end
  window:set_right_status(wezterm.format({
    { Foreground = { Color = '#6b6b6b' } },
    { Text = ' ' .. text .. ' ' },
  }))
end)

-- ===== Font =====
-- System default CJK font (PingFang SC on macOS); let the system pick the best match.
config.font = wezterm.font_with_fallback({
  { family = 'JetBrains Mono', weight = 'Regular' },
  -- Omit explicit CJK font; macOS selects the best one based on locale.
  'Apple Color Emoji',
})

config.font_rules = {
  -- Prevent thin weight: use Regular instead of Light for Half intensity
  {
    intensity = 'Half',
    font = wezterm.font_with_fallback({
      { family = 'JetBrains Mono', weight = 'Regular' },
    }),
  },
  -- Normal italic: disable real italics (keep upright)
  {
    intensity = 'Normal',
    italic = true,
    font = wezterm.font_with_fallback({
      { family = 'JetBrains Mono', weight = 'Regular', italic = false },
    }),
  },
  -- Bold: use Medium weight instead of Heavy
  {
    intensity = 'Bold',
    font = wezterm.font_with_fallback({
      { family = 'JetBrains Mono', weight = 'Medium' },
    }),
  },
}

config.bold_brightens_ansi_colors = false
-- Auto-adjust font size based on screen DPI.
-- Retina (>=150 DPI): 17px, low-resolution external displays (<150 DPI): 15px.
local function get_font_size()
  local success, screens = pcall(function()
    return wezterm.gui.screens()
  end)
  if success and screens and screens.main then
    local dpi = screens.main.effective_dpi or 72
    if dpi < 150 then
      return 15.0  -- Low-resolution external display
    end
  end
  return 17.0  -- Retina default
end

config.font_size = get_font_size()
config.line_height = 1.28
config.cell_width = 1.0
config.harfbuzz_features = { 'calt=0', 'clig=0', 'liga=0' }
config.use_cap_height_to_scale_fallback_fonts = false

config.custom_block_glyphs = true
config.unicode_version = 14

local _, in_app_bundle = wezterm.executable_dir:gsub('MacOS/?$', 'Resources')
if in_app_bundle > 0 then
  config.term = 'kaku'
end

-- ===== Cursor =====
config.default_cursor_style = 'BlinkingBar'
config.cursor_thickness = '2px'
config.cursor_blink_rate = 500
-- Sharp on/off blink without fade animation (like a standard terminal).
config.cursor_blink_ease_in = 'Constant'
config.cursor_blink_ease_out = 'Constant'

-- ===== Scrollback =====
config.scrollback_lines = 10000

-- ===== Mouse =====
config.selection_word_boundary = ' \t\n{}[]()"\'-'  -- Smart selection boundaries

-- ===== Window =====
config.window_padding = {
  left = '40px',
  right = '40px',
  top = '70px',
  bottom = '30px',
}

config.initial_cols = 110
config.initial_rows = 22
-- Mitigate high GPU usage on macOS 26.x by disabling window shadow.
config.window_decorations = "INTEGRATED_BUTTONS|RESIZE|MACOS_FORCE_DISABLE_SHADOW"
config.window_frame = {
  font = wezterm.font({ family = 'JetBrains Mono', weight = 'Regular' }),
  font_size = 13.0,
  active_titlebar_bg = '#15141b',
  inactive_titlebar_bg = '#15141b',
}

config.window_close_confirmation = 'NeverPrompt'
config.window_background_opacity = 1.0
config.text_background_opacity = 1.0

-- ===== Tab Bar =====
config.enable_tab_bar = true
config.tab_bar_at_bottom = true
config.use_fancy_tab_bar = false
config.tab_max_width = 32
config.hide_tab_bar_if_only_one_tab = true
config.show_tab_index_in_tab_bar = true
config.show_new_tab_button_in_tab_bar = false

-- ===== Color Scheme =====
local kaku_theme = {
  -- Background
  foreground = '#edecee',
  background = '#15141b',

  -- Cursor
  cursor_bg = '#a277ff',
  cursor_fg = '#15141b',
  cursor_border = '#a277ff',

  -- Selection
  selection_bg = '#29263c',
  selection_fg = 'none',

  -- Normal colors (ANSI 0-7)
  ansi = {
    '#110f18',  -- black
    '#ff6767',  -- red
    '#61ffca',  -- green
    '#ffca85',  -- yellow
    '#a277ff',  -- blue
    '#a277ff',  -- magenta
    '#61ffca',  -- cyan
    '#edecee',  -- white
  },

  -- Bright colors (ANSI 8-15)
  brights = {
    '#4d4d4d',  -- bright black
    '#ff6767',  -- bright red
    '#61ffca',  -- bright green
    '#ffca85',  -- bright yellow
    '#a277ff',  -- bright blue
    '#a277ff',  -- bright magenta
    '#61ffca',  -- bright cyan
    '#edecee',  -- bright white
  },

  -- Split separator color (increased contrast for better visibility)
  split = '#3d3a4f',

  -- Tab bar colors
  tab_bar = {
    background = '#15141b',
    inactive_tab_edge = '#15141b',

    active_tab = {
      bg_color = '#29263c',
      fg_color = '#edecee',
      intensity = 'Bold',
      underline = 'None',
      italic = false,
      strikethrough = false,
    },

    inactive_tab = {
      bg_color = '#15141b',
      fg_color = '#6b6b6b',
      intensity = 'Normal',
    },

    inactive_tab_hover = {
      bg_color = '#1f1d28',
      fg_color = '#9b9b9b',
      italic = false,
    },

    new_tab = {
      bg_color = '#15141b',
      fg_color = '#6b6b6b',
    },

    new_tab_hover = {
      bg_color = '#1f1d28',
      fg_color = '#9b9b9b',
    },
  },
}

config.color_schemes = config.color_schemes or {}
config.color_schemes['Kaku Theme'] = kaku_theme
if not config.color_scheme then
  config.color_scheme = 'Kaku Theme'
end

-- ===== Shell =====
local user_shell = os.getenv('SHELL')
if user_shell and #user_shell > 0 then
  config.default_prog = { user_shell, '-l' }
else
  config.default_prog = { '/bin/zsh', '-l' }
end

-- ===== macOS Specific =====
-- Keep Left Option as Meta so Alt-based Vim/Neovim keybindings work reliably.
config.send_composed_key_when_left_alt_is_pressed = false
-- Keep Right Option available for composing locale/symbol characters.
config.send_composed_key_when_right_alt_is_pressed = true
config.native_macos_fullscreen_mode = true
config.quit_when_all_windows_are_closed = false

-- ===== Key Bindings =====
config.keys = {
  -- Cmd+K: clear screen + scrollback
  {
    key = 'k',
    mods = 'CMD',
    action = wezterm.action.Multiple({
      wezterm.action.SendKey({ key = 'l', mods = 'CTRL' }),
      wezterm.action.ClearScrollback('ScrollbackAndViewport'),
    }),
  },

  -- Compatibility: keep Cmd+R for existing muscle memory
  {
    key = 'r',
    mods = 'CMD',
    action = wezterm.action.Multiple({
      wezterm.action.SendKey({ key = 'l', mods = 'CTRL' }),
      wezterm.action.ClearScrollback('ScrollbackAndViewport'),
    }),
  },

  -- Cmd+Q: quit
  {
    key = 'q',
    mods = 'CMD',
    action = wezterm.action.QuitApplication,
  },

  -- Cmd+N: new window
  {
    key = 'n',
    mods = 'CMD',
    action = wezterm.action.SpawnWindow,
  },

  -- Cmd+W: close pane > close tab > hide app
  {
    key = 'w',
    mods = 'CMD',
    action = wezterm.action_callback(function(win, pane)
      local mux_win = win:mux_window()
      local tabs = mux_win and mux_win:tabs() or {}
      local current_tab = pane:tab()
      local panes = current_tab and current_tab:panes() or {}
      if #panes > 1 then
        win:perform_action(wezterm.action.CloseCurrentPane { confirm = false }, pane)
      else
        local should_close_tab = (#tabs > 1) or (#wezterm.mux.all_windows() > 1)
        if should_close_tab then
          win:perform_action(wezterm.action.CloseCurrentTab { confirm = false }, pane)
          return
        end
        win:perform_action(wezterm.action.HideApplication, pane)
      end
    end),
  },

  -- Cmd+Shift+W: close current tab
  {
    key = 'w',
    mods = 'CMD|SHIFT',
    action = wezterm.action.CloseCurrentTab({ confirm = false }),
  },

  -- Cmd+T: new tab
  {
    key = 't',
    mods = 'CMD',
    action = wezterm.action.SpawnTab('CurrentPaneDomain'),
  },

  -- Cmd+Shift+A: open Kaku AI config in current pane
  {
    key = 'A',
    mods = 'CMD|SHIFT',
    action = wezterm.action.EmitEvent('run-kaku-ai-config'),
  },

  -- Cmd+Shift+G: launch lazygit in current pane
  {
    key = 'G',
    mods = 'CMD|SHIFT',
    action = wezterm.action.EmitEvent('kaku-launch-lazygit'),
  },

  -- Cmd+Ctrl+F: toggle fullscreen
  {
    key = 'f',
    mods = 'CMD|CTRL',
    action = wezterm.action.ToggleFullScreen,
  },

  -- Cmd+M: minimize window
  {
    key = 'm',
    mods = 'CMD',
    action = wezterm.action.Hide,
  },

  -- Cmd+H: hide application
  {
    key = 'h',
    mods = 'CMD',
    action = wezterm.action.HideApplication,
  },

  -- Cmd+Equal/Minus/0: adjust font size
  {
    key = '=',
    mods = 'CMD',
    action = wezterm.action.IncreaseFontSize,
  },
  {
    key = '-',
    mods = 'CMD',
    action = wezterm.action.DecreaseFontSize,
  },
  {
    key = '0',
    mods = 'CMD',
    action = wezterm.action.ResetFontSize,
  },

  -- Alt+Left / Alt+Right: word jump
  {
    key = 'LeftArrow',
    mods = 'OPT',
    action = wezterm.action.SendKey({ key = 'b', mods = 'ALT' }),
  },
  {
    key = 'RightArrow',
    mods = 'OPT',
    action = wezterm.action.SendKey({ key = 'f', mods = 'ALT' }),
  },

  -- Cmd+Left / Cmd+Right: line start/end
  {
    key = 'LeftArrow',
    mods = 'CMD',
    action = wezterm.action.SendKey({ key = 'a', mods = 'CTRL' }),
  },
  {
    key = 'RightArrow',
    mods = 'CMD',
    action = wezterm.action.SendKey({ key = 'e', mods = 'CTRL' }),
  },

  -- Cmd+Backspace: delete to line start
  {
    key = 'Backspace',
    mods = 'CMD',
    action = wezterm.action.SendKey({ key = 'u', mods = 'CTRL' }),
  },

  -- Alt+Backspace: delete word
  {
    key = 'Backspace',
    mods = 'OPT',
    action = wezterm.action.SendKey({ key = 'w', mods = 'CTRL' }),
  },

  -- Cmd+D: vertical split
  {
    key = 'd',
    mods = 'CMD',
    action = wezterm.action.SplitHorizontal({ domain = 'CurrentPaneDomain' }),
  },

  -- Cmd+Shift+D: horizontal split
  {
    key = 'D',
    mods = 'CMD|SHIFT',
    action = wezterm.action.SplitVertical({ domain = 'CurrentPaneDomain' }),
  },

  -- Cmd+Shift+[ / ]: prev/next tab
  {
    key = '[',
    mods = 'CMD|SHIFT',
    action = wezterm.action.ActivateTabRelative(-1),
  },
  {
    key = ']',
    mods = 'CMD|SHIFT',
    action = wezterm.action.ActivateTabRelative(1),
  },

  -- Cmd+Option+Arrow: navigate between splits
  {
    key = 'LeftArrow',
    mods = 'CMD|OPT',
    action = wezterm.action.ActivatePaneDirection('Left'),
  },
  {
    key = 'RightArrow',
    mods = 'CMD|OPT',
    action = wezterm.action.ActivatePaneDirection('Right'),
  },
  {
    key = 'UpArrow',
    mods = 'CMD|OPT',
    action = wezterm.action.ActivatePaneDirection('Up'),
  },
  {
    key = 'DownArrow',
    mods = 'CMD|OPT',
    action = wezterm.action.ActivatePaneDirection('Down'),
  },

  -- Cmd+1~9: switch tab
  {
    key = '1',
    mods = 'CMD',
    action = wezterm.action.ActivateTab(0),
  },
  {
    key = '2',
    mods = 'CMD',
    action = wezterm.action.ActivateTab(1),
  },
  {
    key = '3',
    mods = 'CMD',
    action = wezterm.action.ActivateTab(2),
  },
  {
    key = '4',
    mods = 'CMD',
    action = wezterm.action.ActivateTab(3),
  },
  {
    key = '5',
    mods = 'CMD',
    action = wezterm.action.ActivateTab(4),
  },
  {
    key = '6',
    mods = 'CMD',
    action = wezterm.action.ActivateTab(5),
  },
  {
    key = '7',
    mods = 'CMD',
    action = wezterm.action.ActivateTab(6),
  },
  {
    key = '8',
    mods = 'CMD',
    action = wezterm.action.ActivateTab(7),
  },
  {
    key = '9',
    mods = 'CMD',
    action = wezterm.action.ActivateTab(8),
  },

  -- Cmd+Enter / Shift+Enter: newline without execute
  {
    key = 'Enter',
    mods = 'CMD',
    action = wezterm.action.SendString('\n'),
  },
  {
    key = 'Enter',
    mods = 'SHIFT',
    action = wezterm.action.SendString('\n'),
  },

  -- Cmd+Shift+Enter: Toggle Pane Zoom (Maximize active pane)
  {
    key = 'Enter',
    mods = 'CMD|SHIFT',
    action = wezterm.action.TogglePaneZoomState,
  },

  -- Cmd+Shift+S: Toggle split direction (horizontal <-> vertical)
  {
    key = 'S',
    mods = 'CMD|SHIFT',
    action = wezterm.action.TogglePaneSplitDirection,
  },

  -- Cmd+Ctrl+Arrows: Resize panes
  {
    key = 'LeftArrow',
    mods = 'CMD|CTRL',
    action = wezterm.action.AdjustPaneSize { 'Left', 5 },
  },
  {
    key = 'RightArrow',
    mods = 'CMD|CTRL',
    action = wezterm.action.AdjustPaneSize { 'Right', 5 },
  },
  {
    key = 'UpArrow',
    mods = 'CMD|CTRL',
    action = wezterm.action.AdjustPaneSize { 'Up', 5 },
  },
  {
    key = 'DownArrow',
    mods = 'CMD|CTRL',
    action = wezterm.action.AdjustPaneSize { 'Down', 5 },
  },


}

-- Copy on select (equivalent to Kitty's copy_on_select)
config.mouse_bindings = {
  {
    event = { Up = { streak = 1, button = 'Left' } },
    mods = 'NONE',
    action = wezterm.action.CompleteSelectionOrOpenLinkAtMouseCursor('ClipboardAndPrimarySelection'),
  },
  {
    event = { Up = { streak = 1, button = 'Left' } },
    mods = 'CMD',
    action = wezterm.action.OpenLinkAtMouseCursor,
  },
}

-- ===== Performance =====
config.enable_scroll_bar = false
config.front_end = 'OpenGL'
config.webgpu_power_preference = 'HighPerformance'
config.animation_fps = 60
config.max_fps = 60
config.status_update_interval = 1000

-- ===== Visuals & Splits =====
-- Split pane gap: gutter = 1 + 2*gap cells, giving ~40px padding on each side
config.split_pane_gap = 2

-- Inactive panes: No dimming (consistent background)
config.inactive_pane_hsb = {
  saturation = 1.0,
  brightness = 1.0,
}

-- Prevent accidental clicks when focusing panes
config.swallow_mouse_click_on_pane_focus = true
config.swallow_mouse_click_on_window_focus = true

-- ===== First Run Experience & Config Version Check =====
wezterm.on('gui-startup', function(cmd)
  local home = os.getenv("HOME")
  local current_version = 9  -- Update this when config changes

  local state_file = home .. "/.config/kaku/state.json"
  local legacy_version_file = home .. "/.config/kaku/.kaku_config_version"
  local legacy_geometry_file = home .. "/.config/kaku/.kaku_window_geometry"
  local is_first_run = false
  local needs_update = false

  local function ensure_state_dir()
    os.execute("mkdir -p " .. home .. "/.config/kaku")
  end

  local function write_state(version, geometry)
    ensure_state_dir()
    local state = {
      config_version = version,
    }
    if geometry and geometry.width and geometry.height then
      state.window_geometry = {
        width = geometry.width,
        height = geometry.height,
      }
    end

    local encoded = nil
    local ok, value = pcall(wezterm.json_encode, state)
    if ok and type(value) == "string" and value ~= "" then
      encoded = value
    end

    local wf = io.open(state_file, "w")
    if wf then
      if encoded then
        wf:write(encoded .. "\n")
      else
        -- Manual JSON fallback when json_encode is unavailable.
        -- Include geometry if present so state is not lost.
        if geometry and geometry.width and geometry.height then
          wf:write(string.format(
            '{\n  "config_version": %d,\n  "window_geometry": {\n    "width": %d,\n    "height": %d\n  }\n}\n',
            version, geometry.width, geometry.height))
        else
          wf:write(string.format('{\n  "config_version": %d\n}\n', version))
        end
      end
      wf:close()
    end
  end

  local function remove_legacy_files()
    os.remove(legacy_version_file)
    os.remove(legacy_geometry_file)
  end

  local function parse_legacy_geometry(raw)
    if not raw or raw == "" then
      return nil
    end

    local values = {}
    for number in raw:gmatch("%d+") do
      values[#values + 1] = tonumber(number)
    end

    if #values >= 4 then
      return {
        width = values[#values - 1],
        height = values[#values],
      }
    elseif #values >= 2 then
      return {
        width = values[1],
        height = values[2],
      }
    end

    return nil
  end

  local function migrate_legacy_state_if_needed()
    local existing_state = io.open(state_file, "r")
    if existing_state then
      existing_state:close()
      return
    end

    local legacy_version = nil
    local legacy_geometry = nil

    local lv = io.open(legacy_version_file, "r")
    if lv then
      legacy_version = tonumber(lv:read("*all"))
      lv:close()
    end

    local lg = io.open(legacy_geometry_file, "r")
    if lg then
      legacy_geometry = parse_legacy_geometry(lg:read("*all"))
      lg:close()
    end

    local has_legacy_markers = legacy_version ~= nil or legacy_geometry ~= nil

    if has_legacy_markers then
      write_state(legacy_version or current_version, legacy_geometry)
      remove_legacy_files()
    end
  end

  migrate_legacy_state_if_needed()

  local user_version = nil
  local state_file_exists = false
  local sf = io.open(state_file, "r")
  if sf then
    state_file_exists = true
    local raw_state = sf:read("*all")
    sf:close()
    if raw_state and raw_state ~= "" then
      local ok, state = pcall(wezterm.json_parse, raw_state)
      if ok and type(state) == "table" then
        user_version = tonumber(state.config_version)
      end
    end
  end

  if not state_file_exists then
    is_first_run = true
  elseif user_version == nil then
    -- Corrupted or manually edited state file: repair with safe defaults.
    write_state(current_version, nil)
    user_version = current_version
  elseif user_version < current_version then
    needs_update = true
  end

  if is_first_run then
    -- First run experience
    ensure_state_dir()

    local resource_dir = wezterm.executable_dir:gsub("MacOS/?$", "Resources")
    local first_run_script = resource_dir .. "/first_run.sh"

    -- Fallback for dev environment
    local f_script = io.open(first_run_script, "r")
    if not f_script then
      first_run_script = wezterm.executable_dir .. "/../../assets/shell-integration/first_run.sh"
    else
      f_script:close()
    end

    wezterm.mux.spawn_window {
      args = { 'bash', first_run_script },
      width = 106,
      height = 22,
    }
    return
  end

  if needs_update then
    -- Apply incremental config updates on version upgrades
    local resource_dir = wezterm.executable_dir:gsub("MacOS/?$", "Resources")
    local update_script = resource_dir .. "/check_config_version.sh"

    -- Fallback for dev environment
    local u_script = io.open(update_script, "r")
    if not u_script then
      update_script = wezterm.executable_dir .. "/../../assets/shell-integration/check_config_version.sh"
    else
      u_script:close()
    end

    wezterm.mux.spawn_window {
      args = { 'bash', update_script },
      width = 106,
      height = 22,
    }
    return
  end

  -- Normal startup
  if not cmd then
    wezterm.mux.spawn_window(cmd or {})
  end
end)

return config
