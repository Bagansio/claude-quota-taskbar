# claude-quota-taskbar

Your Claude plan usage, drawn directly on the Windows 11 taskbar, next to the Widgets/weather button.

```
|  5h  ▮▮▮▮▯▯▯▯▯▯  36% · 3h
|  7d  ▮▮▯▯▯▯▯▯▯▯  23% · 3d
```

- `5h` is the rolling 5-hour session limit, `7d` the weekly limit.
- Each bar has 10 segments. The text shows the percentage used and the time until reset.
- The fill turns amber at 70% and red at 90%. The left rail turns amber while the latest refresh is failing; the last good numbers stay on screen.
- Single ~330 KB `.exe`, no runtime, no console window.

## Install

### Prebuilt
Download `claude-quota-taskbar.exe` from [Releases](../../releases) (or the latest CI artifact) and run it.

### Build from source
On Windows with [Rust](https://rustup.rs) (MSVC toolchain):

```powershell
git clone https://github.com/Bagansio/claude-quota-taskbar.git
cd claude-quota-taskbar
cargo build --release
.\target\release\claude-quota-taskbar.exe
```

Cross-compiling from Linux/WSL (needs `mingw64-gcc` and `rustup target add x86_64-pc-windows-gnu`):

```sh
cargo build --release --target x86_64-pc-windows-gnu
```

### Start at login
Press `Win+R`, run `shell:startup`, and put a shortcut to the `.exe` in that folder.

## Usage

- **Drag** it horizontally to place it anywhere along the taskbar. The position is saved in `%APPDATA%\claude-quota-taskbar\offset`.
- **Right-click** for *Refresh now* and *Quit*.
- Only one instance runs at a time.

## Requirements

- Windows 11
- [Claude Code](https://claude.com/claude-code) installed and logged in with a Pro/Max subscription. The widget reads the OAuth token Claude Code keeps in `%USERPROFILE%\.claude\.credentials.json`. It never writes or refreshes it, and it contacts only `api.anthropic.com`.

> It uses Anthropic's undocumented `/api/oauth/usage` endpoint, which can change or rate-limit you. It polls every 3 minutes (5 minutes after an HTTP 429).

## Troubleshooting

| Message | Meaning |
|---|---|
| `token expired, run claude` | Run any Claude Code command; it renews the token and the widget recovers on its own. |
| `rate limited, retrying` | Too many requests; it backs off automatically. Avoid restarting the app repeatedly. |
| `Claude Code not logged in` | Log in with `claude` first. |
| Nothing visible | Another window may cover that part of the taskbar; drag the widget (the rail is the handle) or check Task Manager for the process. |

## Development

```sh
cargo test --target x86_64-unknown-linux-gnu   # parsing, time maths, formatting (platform independent)
```

| File | Role |
|---|---|
| `src/model.rs` | Pure logic and unit tests: payload parsing, RFC 3339, formatting, segment fills |
| `src/api.rs` | WinHTTP client (system TLS and proxy; no HTTP crate) |
| `src/render.rs` | GDI drawing and layout |
| `src/state.rs` | State shared between fetch thread and UI |
| `src/main.rs` | Window, drag, menu, timers, fetch loop |

The window is a borderless, layered, always-on-top tool window sized to the taskbar. Its background is a colour key, so only the bars and text show. Windows has no API for third-party taskbar widgets, so it re-asserts topmost every 5 seconds.

## License

MIT
