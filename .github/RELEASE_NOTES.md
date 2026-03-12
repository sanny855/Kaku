# V0.7.0 Flow 🌊

<div align="center">
  <img src="https://raw.githubusercontent.com/tw93/Kaku/main/assets/logo.png" alt="Kaku Logo" width="120" height="120" />
  <h1 style="margin: 12px 0 6px;">Kaku V0.7.0</h1>
  <p><em>A fast, out-of-the-box terminal built for AI coding.</em></p>
</div>

### Changelog

1. **Transparency and Visual Consistency**: Improved window_background_opacity rendering for uniform transparency across all UI elements. Added rounded scrollbars, matched toast colors to active pane palette, and stabilized tab bar spacing across fullscreen transitions.

2. **Safer Close Protection**: Added tab and pane close confirmation defaults with refreshed overlays. Double-click zoom no longer interferes with title bar drag for a more predictable window interaction.

3. **Improved Settings TUI**: `kaku config` now has clearer grouped sections, a pinned footer, and a cleaner save flow. Safer config parsing ensures malformed settings won't break the app.

4. **AI Config Upgrades**: Added Antigravity model and quota support with improved parsing and state handling. Background loading and more reliable OAuth token refresh make the AI experience more complete.

5. **Theme-Aware Integrations**: Yazi file manager now syncs its flavor with Kaku's theme selection. JetBrains Mono weights are vendored for better light theme typography.

6. **Smoother File and Editor Workflow**: Enhanced file path link opening and added a remote files shortcut for SSH sessions. Config files now respect `$EDITOR` environment variables.

7. **Shell and Session Reliability**: Hardened shell integration with guard regression fixes. Smart Tab is now limited to Kaku sessions by default, and managed shell/tmux refresh behavior is more stable.

8. **Responsiveness and Edge Cases**: Improved edge resize cursor and window stability. Refined inline tab rename and launcher UX. Hardened Starship RPROMPT fallback for better compatibility.

9. **Pane Input Broadcast**: New broadcast modes enable synchronized input across multiple panes, with safeguards to prevent overlay input from being broadcast by mistake.

### 更新日志

1. **透明度与视觉一致性**：改进窗口背景透明度渲染，确保所有 UI 元素透明效果统一。新增圆角滚动条，toast 颜色跟随当前窗格主题，并优化全屏切换时的标签栏间距稳定性。

2. **更安全的关闭保护**：新增标签页和窗格的关闭确认默认项，重做关闭确认浮层。双击缩放不再干扰标题栏拖拽，窗口交互更可预期。

3. **设置 TUI 体验升级**：`kaku config` 现在有更清晰的分组结构、固定底部操作区和更顺畅的保存流程。更稳健的配置解析确保格式错误不会导致应用崩溃。

4. **AI 配置增强**：新增 Antigravity 模型与额度支持，改进了解析和状态处理逻辑。后台加载和更可靠的 OAuth token 刷新让 AI 体验更完整。

5. **主题感知集成**：Yazi 文件管理器现在会跟随 Kaku 的主题选择同步切换风格。JetBrains Mono 字重已内置，优化浅色主题的字体表现。

6. **更流畅的文件与编辑器工作流**：改进文件路径链接打开体验，为 SSH 会话增加远程文件快捷入口。配置文件现在尊重 `$EDITOR` 环境变量。

7. **Shell 与会话稳定性提升**：加固 shell 集成保护，修复保护回归问题。Smart Tab 默认仅在 Kaku 会话中生效，托管 shell 和 tmux 的刷新行为更稳定。

8. **响应性与边界情况优化**：改进边缘调整大小的光标和窗口稳定性。优化内联标签重命名和启动器交互体验。加固 Starship RPROMPT 回退逻辑，提升兼容性。

9. **窗格输入广播**：新增广播模式支持在多个窗格间同步输入，并补充保护机制，确保浮层输入不会被错误广播到其他窗格。

Special thanks to @frankekn, @crossly, @iwen-conf, and @zxh326 for their contributions to this release.

> https://github.com/tw93/Kaku
