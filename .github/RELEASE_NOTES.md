# V0.12.1 Smoother

<div align="center">
  <img src="https://raw.githubusercontent.com/tw93/Kaku/main/assets/logo.png" alt="Kaku Logo" width="120" height="120" />
  <h1 style="margin: 12px 0 6px;">Kaku V0.12.1</h1>
  <p><em>A fast, out-of-the-box terminal built for AI coding.</em></p>
</div>

### Changelog

1. **Shell Setup**: Upgrading from 0.12 no longer shows `[comment]: command not found` during zsh setup.
2. **Prompt Colors**: Starship, tmux, Powerline, and box-drawing separators keep their intended colors instead of turning into bright blocks.
3. **SmartPrompt**: Cmd+Q no longer asks for confirmation at an idle shell just because background helpers such as `gitstatusd` are running.
4. **Scrolling**: During long AI output, scrolling or resizing no longer jumps to the top of history; Kaku keeps you with the current output.
5. **Smart Tab**: New and bundled configs now let Tab accept the grey autosuggestion first, then fall back to completion. Set `smart_tab_mode = 'completion_first'` to keep the old behavior.
6. **Nightly Package**: The Nightly package is now a signed and notarized DMG, so testing the latest fixes feels like installing a normal release.

### 更新日志

1. **Shell 初始化**：从 0.12 升级时，zsh 设置过程不再出现 `[comment]: command not found` 报错。
2. **提示符颜色**：Starship、tmux、Powerline 和 box-drawing 分隔符会保留原本颜色，不再被提亮成突兀色块。
3. **SmartPrompt**：停在空闲 shell 时，Cmd+Q 不会再因为 `gitstatusd` 这类后台进程弹确认。
4. **滚动行为**：AI 长输出过程中滚动或调整窗口，不会再突然跳到历史最顶端，会继续跟住当前输出。
5. **Smart Tab**：新配置和 bundled 默认现在改为 Tab 优先接受灰色建议，没有建议时再回退到补全；想保留旧行为可设置 `smart_tab_mode = 'completion_first'`。
6. **Nightly 包**：Nightly 预览包现在是签名并公证过的 DMG，安装和验证最新修复时更接近正式版体验。

> https://github.com/tw93/Kaku
