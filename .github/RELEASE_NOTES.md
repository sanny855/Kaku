# V0.10.0 Chat 🪄

<div align="center">
  <img src="https://raw.githubusercontent.com/tw93/Kaku/main/assets/logo.png" alt="Kaku Logo" width="120" height="120" />
  <h1 style="margin: 12px 0 6px;">Kaku V0.10.0</h1>
  <p><em>A fast, out-of-the-box terminal built for AI coding.</em></p>
</div>

### Changelog

1. **AI Chat Panel**: Press `Cmd+L` for a terminal-native AI chat with streaming Markdown, syntax highlighting, shell context, project tools, web search, and memory.
2. **`k` CLI**: The same AI engine in an alternate-screen TUI, with theme detection and safer cancel and approval behavior.
3. **Smart Close Protection**: `Cmd+W` and `Cmd+Shift+W` now ask before killing a pane that runs claude, codex, cursor-agent, gemini, vim, cargo, npm, or any non-shell process. Bare shells still close silently.
4. **AI Configuration**: Assistant settings now use Simple and Deep models, with live model loading, proxy-aware requests, OAuth setup, and broader provider responses.
5. **AI Safety and Context**: Stricter shell approvals, sensitive-path guards covering search and project tools, tighter file write and patch limits, failed-command context, and clearer parse errors.
6. **Window Snapshots**: Kaku auto-saves multi-tab and multi-pane layouts. Restore with `Cmd+Option+Shift+T`, Shell → Restore Previous Window, or the Command Palette.
7. **macOS and Terminal UX**: Fixed fullscreen crashes, display races, resize gaps, tab drag state, cursor reflow, links, selection, and TUI copy.
8. **Updates, Shell, and Performance**: Background downloads with fail-closed checksums, better proxy and MacPorts detection, cached shell state, lighter startup.

### 更新日志

1. **AI 对话面板**：按 `Cmd+L` 打开终端内 AI Chat，支持流式 Markdown、语法高亮、shell 上下文、项目工具、网页搜索和本地记忆。
2. **`k` CLI**：新增 `k` 二进制，把同一套 AI 引擎放进 alternate-screen TUI，主题识别、取消和审批语义都更稳。
3. **智能关闭保护**：`Cmd+W` 和 `Cmd+Shift+W` 在 pane 里跑着 claude、codex、cursor-agent、gemini、vim、cargo、npm 这类有状态进程时会先弹确认，bare shell 仍然直接关。
4. **AI 配置**：Assistant 设置改为 Simple Model 和 Deep Model，支持在线模型加载、代理感知请求、OAuth 配置，以及更多 provider 响应格式。
5. **AI 安全与上下文**：shell 审批、敏感路径保护扩展到搜索类工具、文件写入与 patch 上限收紧、失败命令上下文和解析错误都更稳。
6. **窗口快照**：Kaku 自动保存多 tab、多 pane 布局，需要时按 `Cmd+Option+Shift+T`，或从 Shell → Restore Previous Window、命令面板恢复。
7. **macOS 与终端体验**：修复全屏崩溃和卡住、显示器竞态、resize 缝隙、tab 拖拽残留、光标 reflow、链接、选择和 TUI 复制。
8. **更新、Shell 与性能**：更新改为后台下载，checksum 失败时关闭风险路径，代理与 MacPorts 检测更稳，shell 状态缓存，冷启动更轻。

Special thanks to @s010s, @SherlockSalvatore, @darion-yaphet, @ddotz, @beautifulrem, @yxspace, and @fanweixiao for their contributions to this release.

> https://github.com/tw93/Kaku
