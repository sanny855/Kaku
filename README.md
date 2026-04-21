<div align="center">
  <img src="https://gw.alipayobjects.com/zos/k/6h/dwarf.svg" width="120" />
  <h1>Kaku</h1>
  <p><em>A fast, out-of-the-box terminal built for AI coding.</em></p>
</div>

<p align="center">
  <a href="https://github.com/tw93/Kaku/stargazers"><img src="https://img.shields.io/github/stars/tw93/Kaku?style=flat-square" alt="Stars"></a>
  <a href="https://github.com/tw93/Kaku/releases"><img src="https://img.shields.io/github/v/tag/tw93/Kaku?label=version&style=flat-square" alt="Version"></a>
  <a href="LICENSE.md"><img src="https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square" alt="License"></a>
  <a href="https://github.com/tw93/Kaku/commits"><img src="https://img.shields.io/github/commit-activity/m/tw93/Kaku?style=flat-square" alt="Commits"></a>
  <a href="https://twitter.com/HiTw93"><img src="https://img.shields.io/badge/follow-Tw93-red?style=flat-square&logo=Twitter" alt="Twitter"></a>
</p>

<p align="center">
  <img src="assets/kaku.jpg" alt="Kaku Screenshot" width="1000" />
  <br/>
  Kaku is a deeply customized fork of <a href="https://github.com/wez/wezterm">WezTerm</a>, designed for an out-of-the-box experience.
</p>

Part of a trilogy: [Kaku](https://github.com/tw93/Kaku) (書く) writes code, [Waza](https://github.com/tw93/Waza) (技) drills habits, [Kami](https://github.com/tw93/Kami) (紙) ships documents. Think of them as a family: Kaku is the dad, Waza the big sister, Kami the little sister.

## Features

- **Zero Config**: Defaults with JetBrains Mono, macOS font rendering, and low-res font sizing.
- **Theme-Aware Experience**: Auto-switches between dark and light modes with macOS, with tuned selection colors, font weight, and practical color overrides support.
- **Curated Shell Suite**: Built-in zsh plugins with optional CLI tools for prompt, diff, and navigation workflows.
- **Fast & Lightweight**: 40% smaller binary, instant startup, lazy loading, stripped-down GPU-accelerated core.
- **WezTerm-Compatible Config**: Use WezTerm's Lua config directly with full API compatibility and no migration.
- **Polished Defaults**: Copy on select, clickable file paths, history peek from full-screen apps, pane input broadcast, and visual bell on background tab completion.

## Quick Start

1. [Download Kaku DMG](https://github.com/tw93/Kaku/releases/latest) & Drag to Applications
2. Or install with Homebrew: `brew install tw93/tap/kakuku`
3. Open Kaku. The app is notarized by Apple, so it opens without security warnings
4. On first launch, Kaku will automatically set up your shell environment

## Usage Guide

| Action | Shortcut |
| :--- | :--- |
| New Tab | `Cmd + T` |
| New Window | `Cmd + N` |
| Close Tab/Pane | `Cmd + W` |
| Navigate Tabs | `Cmd + Shift + [` / `]` or `Cmd + 1–9` |
| Navigate Panes | `Cmd + Opt + Arrows` |
| Split Pane Vertical | `Cmd + D` |
| Split Pane Horizontal | `Cmd + Shift + D` |
| Open Settings Panel | `Cmd + ,` |
| AI Panel | `Cmd + Shift + A` |
| Apply AI Suggestion | `Cmd + Shift + E` |
| Open Lazygit | `Cmd + Shift + G` |
| Yazi File Manager | `Cmd + Shift + Y` or `y` |
| Clear Screen | `Cmd + K` |

Full keybinding reference: [docs/keybindings.md](docs/keybindings.md)

## Kaku AI

Kaku has a built-in assistant with two modes and a settings page for AI coding tools.

- **Error recovery**: When a command fails, Kaku automatically suggests a fix. Press `Cmd + Shift + E` to apply.
- **Natural language to command**: Type `# <description>` at the prompt and press Enter. Kaku sends the query to the LLM and injects the resulting command back into the prompt, ready to review and run.
- **AI Tools Config**: Manage settings for Claude Code, Codex, Gemini CLI, Copilot CLI, Kimi Code, and more.

### Provider Presets

Select a provider in `kaku ai` to auto-fill the base URL and models:

| Provider | Base URL | Models |
| :--- | :--- | :--- |
| OpenAI | `https://api.openai.com/v1` | (free text) |
| Custom | (manual) | (manual) |

Full AI assistant docs: [docs/features.md](docs/features.md)

## Why Kaku?

I heavily rely on the CLI for both work and personal projects. Tools I've built, like [Mole](https://github.com/tw93/mole) and [Pake](https://github.com/tw93/pake), reflect this.

I used Alacritty for years and learned to value speed and simplicity. As my workflow shifted toward AI-assisted coding, I wanted stronger tab and pane ergonomics. I also explored Kitty, Ghostty, Warp, and iTerm2. Each is strong in different areas, but I still wanted a setup that matched my own balance of performance, defaults, and control.

WezTerm is robust and highly hackable, and I am grateful for its engine and ecosystem. Kaku builds on that foundation with practical defaults for day one use, while keeping full Lua-based customization and a fast, lightweight feel.

So I built Kaku to be that environment: fast, polished, and ready to work.

### Performance

| Metric | Upstream | Kaku | Methodology |
| :--- | :--- | :--- | :--- |
| **Executable Size** | ~67 MB | ~40 MB | Aggressive symbol stripping & feature pruning |
| **Resources Volume** | ~100 MB | ~80 MB | Asset optimization & lazy-loaded assets |
| **Launch Latency** | Standard | Instant | Just-in-time initialization |
| **Shell Bootstrap** | ~200ms | ~100ms | Optimized environment provisioning |

## FAQ

**Is there a Windows or Linux version?** Not currently. Kaku is macOS-only for now.

**Can I use transparent windows?** Yes, set `config.window_background_opacity` in `~/.config/kaku/kaku.lua`.

**The `kaku` command is missing.** Run `/Applications/Kaku.app/Contents/MacOS/kaku init --update-only && exec zsh -l`, then `kaku doctor`.

Full FAQ: [docs/faq.md](docs/faq.md)

## Docs

- [Keybindings](docs/keybindings.md) - full shortcut reference
- [Features](docs/features.md) - AI assistant, lazygit, yazi, remote files, shell suite
- [Configuration](docs/configuration.md) - themes, fonts, custom keybindings, Lua API
- [CLI Reference](docs/cli.md) - `kaku ai`, `kaku config`, `kaku doctor`, and more
- [FAQ](docs/faq.md) - common questions and troubleshooting

## Contributors

Big thanks to all contributors who helped build Kaku. Go follow them! ❤️

<a href="https://github.com/tw93/Kaku/graphs/contributors">
  <img src="./CONTRIBUTORS.svg?v=2" width="1000" />
</a>

## Support

- If Kaku helped you, [share it](https://twitter.com/intent/tweet?url=https://github.com/tw93/Kaku&text=Kaku%20-%20A%20fast%20terminal%20built%20for%20AI%20coding.) with friends or give it a star.
- Got ideas or bugs? Open an issue or PR, feel free to contribute your best AI model.
- I have two cats, TangYuan and Coke. If you think Kaku delights your life, you can feed them <a href="https://miaoyan.app/cats.html?name=Kaku" target="_blank">canned food 🥩</a>.

<a href="https://miaoyan.app/cats.html?name=Kaku"><img src="https://cdn.jsdelivr.net/gh/tw93/MiaoYan@main/assets/sponsors.svg" width="1000" loading="lazy" /></a>

## License

MIT License, feel free to enjoy and participate in open source.
