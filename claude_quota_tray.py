"""Claude quota system-tray icon. Shows session % on the icon; hover for details."""
import json, os, threading, time, urllib.request, urllib.error
from datetime import datetime, timezone
import pystray
from PIL import Image, ImageDraw, ImageFont

CRED = os.path.expanduser("~/.claude/.credentials.json")
URL = "https://api.anthropic.com/api/oauth/usage"
POLL_S = 60
state = {"session": None, "weekly": None, "s_reset": "", "w_reset": "", "err": ""}


def fetch():
    tok = json.load(open(CRED))["claudeAiOauth"]["accessToken"]
    req = urllib.request.Request(URL, headers={
        "Authorization": "Bearer " + tok, "anthropic-beta": "oauth-2025-04-20",
        "User-Agent": "claude-quota-tray/1.0"})
    return json.load(urllib.request.urlopen(req, timeout=15))


def until(iso):
    if not iso:
        return ""
    s = int((datetime.fromisoformat(iso) - datetime.now(timezone.utc)).total_seconds())
    if s <= 0:
        return "resetting"
    d, r = divmod(s, 86400); h, r = divmod(r, 3600); m = r // 60
    return f"{d}d {h}h" if d else f"{h}h {m}m" if h else f"{m}m"


def icon_image():
    p = state["session"]
    col = (90, 90, 90) if p is None else (217, 71, 63) if p >= 90 else (224, 160, 48) if p >= 70 else (217, 119, 87)
    im = Image.new("RGBA", (64, 64), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    d.rounded_rectangle((0, 0, 63, 63), 12, fill=col)
    txt = "!" if state["err"] and p is None else "–" if p is None else str(int(round(p)))
    try:
        f = ImageFont.truetype("segoeuib.ttf", 44 if len(txt) < 3 else 34)
    except Exception:
        f = ImageFont.load_default()
    d.text((32, 34), txt, font=f, fill="white", anchor="mm")
    return im


def tooltip():
    if state["session"] is None:
        return "Claude: " + (state["err"] or "loading…")
    t = (f"Claude\nSession: {state['session']:.0f}% (resets in {state['s_reset']})\n"
         f"Weekly: {state['weekly']:.0f}% (resets in {state['w_reset']})")
    return t + ("\n" + state["err"] if state["err"] else "")


def update(icon):
    try:
        d = fetch()
        L = {l["kind"]: l for l in d.get("limits", [])}
        s = L.get("session") or {"percent": d["five_hour"]["utilization"], "resets_at": d["five_hour"]["resets_at"]}
        w = L.get("weekly_all") or {"percent": d["seven_day"]["utilization"], "resets_at": d["seven_day"]["resets_at"]}
        state.update(session=s["percent"], weekly=w["percent"], s_reset=until(s["resets_at"]),
                     w_reset=until(w["resets_at"]), err="")
    except urllib.error.HTTPError as e:
        state["err"] = "token expired – use Claude Code" if e.code == 401 else f"HTTP {e.code}"
    except Exception:
        state["err"] = "offline / error"
    icon.icon = icon_image()
    icon.title = tooltip()
    icon.update_menu()


def loop(icon):
    icon.visible = True
    while True:
        update(icon)
        time.sleep(POLL_S)


def main():
    menu = pystray.Menu(
        pystray.MenuItem(lambda i: f"Session: {state['session']:.0f}% · resets in {state['s_reset']}" if state["session"] is not None else "Session: –", None, enabled=False),
        pystray.MenuItem(lambda i: f"Weekly: {state['weekly']:.0f}% · resets in {state['w_reset']}" if state["weekly"] is not None else "Weekly: –", None, enabled=False),
        pystray.Menu.SEPARATOR,
        pystray.MenuItem("Refresh now", lambda i, _: threading.Thread(target=update, args=(i,), daemon=True).start(), default=True),
        pystray.MenuItem("Quit", lambda i, _: i.stop()))
    icon = pystray.Icon("claude_quota", icon_image(), "Claude: loading…", menu)
    icon.run(setup=lambda i: threading.Thread(target=loop, args=(i,), daemon=True).start())


if __name__ == "__main__":
    main()
