# claude-quota-taskbar

Shows your Claude plan usage (5-hour session and weekly) on the Windows 11 taskbar, next to the Widgets/weather button: a colored dot plus two lines of text.

```
●  Session 10%  ·  3h12m
   Weekly 1%    ·  4d2h
```

The dot goes orange, then amber at 70% and red at 90%.

Also included: `claude_quota_tray.py`, a system-tray alternative that shows the session % on the icon and details on hover.

## How it works

It reads the OAuth token that Claude Code stores in `%USERPROFILE%\.claude\.credentials.json` and calls Anthropic's usage endpoint (`https://api.anthropic.com/api/oauth/usage`) every 3 minutes. Nothing is written or sent anywhere else, and the token is never refreshed or modified. The repo contains no secrets.

> The endpoint is undocumented and may change or rate-limit you (HTTP 429). The widget keeps retrying on its own.

## Requirements

- Windows 11
- [Claude Code](https://claude.com/claude-code) installed and logged in (a Pro/Max subscription)
- Python 3.10+ with tkinter (the standard python.org installer includes it)

## Install

```powershell
git clone https://github.com/Bagansio/claude-quota-taskbar.git
cd claude-quota-taskbar
wscript start.vbs
```

The widget appears at the left of the taskbar. Drag it horizontally to place it where you want (the position is saved to `pos.json`). Right-click for **Refresh now** / **Quit**.

### Start at login

Press `Win+R`, run `shell:startup`, and put a shortcut to `start.vbs` in that folder.

### Tray version instead

```powershell
pip install -r requirements-tray.txt
pythonw claude_quota_tray.py
```

New tray icons are hidden under the `^` arrow; drag it onto the taskbar to pin it.

## Troubleshooting

| Symptom | Fix |
|---|---|
| "Token expired" | Run any Claude Code command; it renews the token. |
| "HTTP 429" / "Rate limited" | Wait a few minutes; avoid restarting the widget repeatedly. |
| Nothing visible | Run `python claude_quota_bar.py` in a terminal to see errors. Check `python -c "import tkinter"`. |
| Overlaps other icons | Drag it, or edit the default offset in `claude_quota_bar.py`. |

## Notes

Windows does not let third-party apps add to the real Widgets button, so this is a borderless, transparent always-on-top window sized to the taskbar. It re-asserts itself on top every few seconds. If you use a light taskbar theme, change `FG`/`MUT` at the top of `claude_quota_bar.py`.

MIT licensed.
