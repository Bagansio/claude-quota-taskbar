# claude-quota-taskbar

Your Claude plan usage on your desktop bar: embedded in the **Windows 11 taskbar**, or as a **Waybar module on Hyprland** (and other Wayland compositors that use Waybar: Sway, river, niri, ...).

```
|  5h  ▮▮▮▮▯▯▯▯▯▯  36% · 3h
|  7d  ▮▮▯▯▯▯▯▯▯▯  23% · 3d
```

- `5h` is the rolling 5-hour session limit, `7d` the weekly limit.
- Each bar has 10 segments. The text shows the percentage used and the time until reset.
- The fill turns amber at 70% and red at 90%. If a refresh fails, the last good numbers stay on screen and the readout is marked stale.
- Both frontends read the OAuth token Claude Code already stores. They never write or refresh it, and they only contact `api.anthropic.com`.

| | Windows | Hyprland / Waybar |
|---|---|---|
| Binary | `claude-quota-taskbar.exe` (~330 KB) | `claude-quota-waybar` (~420 KB) |
| Draws with | GDI, as a child window of the taskbar | Waybar custom module (JSON + Pango) |
| HTTP | WinHTTP (system TLS and proxy) | system `curl` (system TLS, proxy env vars) |
| Credentials | `%USERPROFILE%\.claude\.credentials.json` | `~/.claude/.credentials.json` |

`$CLAUDE_CONFIG_DIR` overrides the `.claude` directory on both, as it does for Claude Code.

## Windows

### Install
Download `claude-quota-taskbar.exe` from [Releases](../../releases) (or the latest CI artifact) and run it.

To build from source, on Windows with [Rust](https://rustup.rs) (MSVC toolchain):

```powershell
git clone https://github.com/Bagansio/claude-quota-taskbar.git
cd claude-quota-taskbar
cargo build --release -p claude-quota-taskbar
.\target\release\claude-quota-taskbar.exe
```

To cross-compile from Linux/WSL (needs `mingw64-gcc` and `rustup target add x86_64-pc-windows-gnu`):

```sh
cargo build --release -p claude-quota-taskbar --target x86_64-pc-windows-gnu
```

To start it at login, press `Win+R`, run `shell:startup`, and put a shortcut to the `.exe` in that folder.

### Usage
- **Drag** it horizontally to place it anywhere along the taskbar. The position is saved in `%APPDATA%\claude-quota-taskbar\offset`.
- **Right-click** for *Refresh now* and *Quit*.
- Only one instance runs at a time.
- The left rail turns amber while the latest refresh is failing.

## Hyprland (Waybar)

Hyprland has no taskbar of its own; the bar is almost always [Waybar](https://github.com/Alexays/Waybar), so this ships as a Waybar custom module instead of a separate window. Clicking, theming, and placement work like any other module.

### Install
Requires `curl` and Waybar. Download `claude-quota-waybar-x86_64-linux` from [Releases](../../releases), or build it:

```sh
git clone https://github.com/Bagansio/claude-quota-taskbar.git
cd claude-quota-taskbar
cargo build --release -p claude-quota-waybar
install -Dm755 target/release/claude-quota-waybar ~/.local/bin/claude-quota-waybar
```

### Configure
Add `"custom/claude"` to one of your `modules-left` / `modules-center` / `modules-right` arrays, then add the module from [`contrib/waybar/config.jsonc`](contrib/waybar/config.jsonc):

```jsonc
"custom/claude": {
    "exec": "claude-quota-waybar",
    "return-type": "json",
    "restart-interval": 30,
    "on-click": "claude-quota-waybar --refresh",
    "tooltip": true
}
```

Optionally append [`contrib/waybar/style.css`](contrib/waybar/style.css) to `~/.config/waybar/style.css`, then reload Waybar with `killall -SIGUSR2 waybar`.

The bar shows `5h ■■■■□□□□□□ 36% · 3h   7d ■■□□□□□□□□ 23% · 3d`. The tooltip shows exact reset countdowns and when the data was last updated.

| Flag | Effect |
|---|---|
| *(none)* | Run continuously and print a new JSON line whenever the readout changes. |
| `--compact` | Percentages and reset times only, without the segment bars. |
| `--plain` | No Pango colours, so you can style it entirely through CSS classes. |
| `--once` | Print one line and exit, for `"interval"`-style modules. |
| `--refresh` | Ask running instances to refetch now. Use it as `on-click`; it runs at most once every 15 s. |

CSS classes are `ok`, `warn` (≥70%), and `critical` (≥90%), based on the higher of the two limits, plus `stale` when the latest refresh failed. Before any data has loaded, the class is `loading` or `error` instead. `percentage` is set to the higher limit, so `format-icons` works too.

Waybar starts one process per bar, so a multi-monitor setup runs several instances. They share state in `$XDG_RUNTIME_DIR/claude-quota-waybar/` (mode 0700, tmpfs), and only one of them fetches per interval. That directory only ever holds percentages, reset times, and error messages, never the token. The token goes to `curl` over stdin, not as an argument, so it doesn't appear in `ps`.

## Requirements

- [Claude Code](https://claude.com/claude-code) installed and logged in with a Pro/Max subscription.
- Windows 11, or Linux with Waybar and `curl`.

> It uses Anthropic's undocumented `/api/oauth/usage` endpoint, which can change or rate-limit you. It polls every 3 minutes (5 minutes after an HTTP 429).

## Troubleshooting

| Message | Meaning |
|---|---|
| `token expired, run claude` | Run any Claude Code command; it renews the token and the widget recovers on its own. |
| `rate limited, retrying` | Too many requests; it backs off automatically. Avoid restarting the app repeatedly. |
| `Claude Code not logged in` | Log in with `claude` first. |
| `curl not found` | (Linux) Install `curl`. |
| Nothing visible (Windows) | Another window may cover that part of the taskbar; drag the widget (the rail is the handle) or check Task Manager for the process. |
| Nothing visible (Waybar) | Run `claude-quota-waybar --once` in a terminal; check that `custom/claude` is listed in a `modules-*` array. |

## Development

The repository is a Cargo workspace:

```
crates/core      claude-quota-core: credentials, response handling, parsing, formatting, polling policy (unit-tested)
crates/windows   claude-quota-taskbar: Windows taskbar widget (WinHTTP + GDI)
crates/waybar    claude-quota-waybar: Waybar module (curl + JSON/Pango output, shared cache)
contrib/waybar   example Waybar config and CSS
```

```sh
cargo test --workspace     # on Linux; the Windows crate compiles to a stub there
cargo test -p claude-quota-core
```

| File | Role |
|---|---|
| `crates/core/src/model.rs` | Payload parsing, RFC 3339, formatting, segment fills |
| `crates/core/src/fetch.rs` | Credentials lookup, expiry check, HTTP status mapping; takes the HTTP GET as a function |
| `crates/windows/src/api.rs` | WinHTTP client |
| `crates/windows/src/render.rs` | GDI drawing and layout |
| `crates/windows/src/main.rs` | Window, drag, menu, timers, fetch loop |
| `crates/waybar/src/http.rs` | `curl` client (token over stdin) |
| `crates/waybar/src/output.rs` | Waybar JSON, Pango markup, CSS classes |
| `crates/waybar/src/cache.rs` | State shared between instances, fetch lock, refresh requests |
| `crates/waybar/src/main.rs` | CLI and run loop |

To add another frontend (e.g. eww, ags, a macOS menu bar), depend on `claude-quota-core`, supply an `http_get(token) -> (status, body)`, and draw the `Usage`.

On Windows, the widget is a borderless layered child window of the taskbar itself (`Shell_TrayWnd`), so Start, flyouts, and full-screen apps cover it like any other taskbar content. Its background is a colour key, so only the bars and text show. Layered child windows need the embedded manifest (`app.manifest`). If no taskbar window is found, it falls back to a topmost popup. Windows has no official API for third-party taskbar widgets.

## License

MIT
