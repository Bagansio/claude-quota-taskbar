"""Claude quota text embedded in the Windows taskbar (borderless, transparent).
Drag horizontally to reposition (saved). Right-click: Refresh / Quit."""
import ctypes, json, os, threading, tkinter as tk, urllib.request, urllib.error
from datetime import datetime, timezone

ctypes.windll.shcore.SetProcessDpiAwareness(2)
U = ctypes.windll.user32
CRED = os.path.expanduser("~/.claude/.credentials.json")
POS = os.path.join(os.path.dirname(os.path.abspath(__file__)), "pos.json")
URL = "https://api.anthropic.com/api/oauth/usage"
KEY, FG, MUT = "#010101", "#ffffff", "#a8a8a8"
OK, WARN, BAD = "#d97757", "#e0a030", "#d9473f"


def fetch():
    tok = json.load(open(CRED))["claudeAiOauth"]["accessToken"]
    req = urllib.request.Request(URL, headers={
        "Authorization": "Bearer " + tok, "anthropic-beta": "oauth-2025-04-20",
        "User-Agent": "claude-quota-bar/1.0"})
    return json.load(urllib.request.urlopen(req, timeout=15))


def until(iso):
    s = int((datetime.fromisoformat(iso) - datetime.now(timezone.utc)).total_seconds())
    if s <= 0:
        return "0m"
    d, r = divmod(s, 86400); h, r = divmod(r, 3600); m = r // 60
    return f"{d}d{h}h" if d else f"{h}h{m:02d}m" if h else f"{m}m"


class R(ctypes.Structure):
    _fields_ = [("l", ctypes.c_long), ("t", ctypes.c_long), ("r", ctypes.c_long), ("b", ctypes.c_long)]


def taskbar_rect():
    h = U.FindWindowW("Shell_TrayWnd", None)
    r = R(); U.GetWindowRect(h, ctypes.byref(r))
    return r.l, r.t, r.r - r.l, r.b - r.t


class App:
    W = 190

    def __init__(self):
        self.root = root = tk.Tk()
        root.overrideredirect(True)
        root.attributes("-topmost", True)
        root.configure(bg=KEY)
        root.attributes("-transparentcolor", KEY)
        self.tx, self.ty, self.tw, self.th = taskbar_rect()
        try:
            self.x = json.load(open(POS))[0]
        except Exception:
            self.x = self.tx + 190
        root.geometry(f"{self.W}x{self.th}+{self.x}+{self.ty}")
        self.c = tk.Canvas(root, width=self.W, height=self.th, bg=KEY, highlightthickness=0)
        self.c.pack()
        self.data = None; self.err = ""
        menu = tk.Menu(root, tearoff=0)
        menu.add_command(label="Refresh now", command=self.refresh)
        menu.add_command(label="Quit", command=root.destroy)
        self.c.bind("<Button-3>", lambda e: menu.tk_popup(e.x_root, e.y_root))
        self.c.bind("<Button-1>", lambda e: setattr(self, "dx", e.x_root - self.x))
        self.c.bind("<B1-Motion>", self.drag)
        self.c.bind("<ButtonRelease-1>", lambda e: json.dump([self.x], open(POS, "w")))
        self.draw(); self.tick(); self.keep_on_top()

    def drag(self, e):
        self.x = e.x_root - self.dx
        self.root.geometry(f"+{self.x}+{self.ty}")

    def keep_on_top(self):
        self.root.attributes("-topmost", True); self.root.lift()
        self.root.after(5000, self.keep_on_top)

    def refresh(self):
        threading.Thread(target=self.work, daemon=True).start()

    def work(self):
        try:
            d = fetch(); L = {l["kind"]: l for l in d.get("limits", [])}
            s = L.get("session") or {"percent": d["five_hour"]["utilization"], "resets_at": d["five_hour"]["resets_at"]}
            w = L.get("weekly_all") or {"percent": d["seven_day"]["utilization"], "resets_at": d["seven_day"]["resets_at"]}
            res, err = (s, w), ""
        except urllib.error.HTTPError as e:
            res, err = None, "Token expired" if e.code == 401 else f"HTTP {e.code}"
        except Exception:
            res, err = None, "Offline"
        self.root.after(0, self.set, res, err)

    def set(self, res, err):
        if res: self.data = res
        self.err = err
        self.draw()
        if err == "HTTP 429" and not self.data:
            self.err = "Rate limited, retrying"

    def draw(self):
        c, h = self.c, self.th
        c.delete("all")
        p = self.data[0]["percent"] if self.data else None
        col = "#707070" if p is None else BAD if p >= 90 else WARN if p >= 70 else OK
        r = 11
        c.create_oval(8, h / 2 - r, 8 + 2 * r, h / 2 + r, fill=col, outline="")
        f1, f2 = ("Segoe UI", 9, "bold"), ("Segoe UI", 9)
        if self.data:
            (s, w) = self.data
            t1 = f"Session {s['percent']:.0f}%  ·  {until(s['resets_at'])}"
            t2 = f"Weekly {w['percent']:.0f}%  ·  {until(w['resets_at'])}"
        else:
            t1, t2 = "Claude", self.err or "loading…"
        if self.err and self.data:
            t2 += f"  ({self.err})"
        c.create_text(8 + 2 * r + 8, h / 2 - 9, text=t1, anchor="w", fill=FG, font=f1)
        c.create_text(8 + 2 * r + 8, h / 2 + 9, text=t2, anchor="w", fill=MUT, font=f2)

    def tick(self):
        self.refresh(); self.draw()
        wait = 300_000 if self.err == "HTTP 429" else 180_000
        self.root.after(wait, self.tick)


if __name__ == "__main__":
    App().root.mainloop()
