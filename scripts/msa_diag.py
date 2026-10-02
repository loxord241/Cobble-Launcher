import json, time, urllib.request, urllib.parse, socket, subprocess, sys

CLIENT = "349c8527-95ae-470a-8500-295c5d7721fe"
PORT = 53111
AUTH = f"https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize?client_id={CLIENT}&response_type=code&redirect_uri=http%3A%2F%2Flocalhost%3A{PORT}&scope=XboxLive.signin%20offline_access&prompt=none&domain_hint=consumers"

def log(*a): print(*a, flush=True)

def post_json(url, obj, headers=None):
    data = json.dumps(obj).encode()
    h = {"Content-Type": "application/json", "Accept": "application/json"}
    if headers: h.update(headers)
    req = urllib.request.Request(url, data=data, headers=h, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            return r.status, json.loads(r.read().decode()), dict(r.headers)
    except urllib.error.HTTPError as e:
        body = e.read().decode(errors="replace")
        try: body = json.loads(body)
        except Exception: pass
        return e.code, body, dict(e.headers)

# 1. code
srv = socket.socket()
srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
srv.bind(("127.0.0.1", PORT))
srv.listen(1)
subprocess.run(["powershell", "-NoProfile", "-Command", f"Start-Process '{AUTH}'"])
log(f"[1] слушаю localhost:{PORT}, открыл браузер (prompt=none)")
code = None
srv.settimeout(60)
while code is None:
    conn, _ = srv.accept()
    req = conn.recv(65536).decode(errors="replace")
    line = req.splitlines()[0] if req else ""
    page = b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nConnection: close\r\n\r\nok"
    try:
        path = line.split(" ")[1]
        q = urllib.parse.parse_qs(urllib.parse.urlparse(path).query)
        if "code" in q: code = q["code"][0]
        elif "error" in q: log("[1] браузер вернул ошибку:", q.get("error"), q.get("error_description"))
    except Exception as e:
        log("[1] parse fail:", e, line[:100])
    conn.sendall(page); conn.close()
log(f"[1] code получен: len={len(code)} head={code[:10]}")

# 2. exchange
data = urllib.parse.urlencode({
    "client_id": CLIENT, "grant_type": "authorization_code", "code": code,
    "redirect_uri": f"http://localhost:{PORT}", "scope": "XboxLive.signin offline_access",
}).encode()
req = urllib.request.Request(f"https://login.microsoftonline.com/consumers/oauth2/v2.0/token", data=data, method="POST")
try:
    with urllib.request.urlopen(req, timeout=30) as r:
        tok = json.loads(r.read().decode())
except urllib.error.HTTPError as e:
    log("[2] ОБМЕН УПАЛ:", e.code, e.read().decode(errors="replace")[:300]); sys.exit(1)
ms = tok["access_token"]
log(f"[2] exchange ok: ms_token len={len(ms)}")

# 2b. clock skew vs server
req = urllib.request.Request("https://user.auth.xboxlive.com/user/authenticate", method="OPTIONS")
try:
    with urllib.request.urlopen(req, timeout=15) as r:
        d = r.headers.get("Date")
        if d:
            from email.utils import parsedate_to_datetime
            skew = time.time() - parsedate_to_datetime(d).timestamp()
            log(f"[2b] сдвиг часов с сервером: {skew:+.1f} сек")
except Exception as e:
    log("[2b] date check fail:", e)

# 3. XBL
s, xbl, _ = post_json("https://user.auth.xboxlive.com/user/authenticate", {
    "Properties": {"AuthMethod": "RPS", "SiteName": "user.auth.xboxlive.com", "RpsTicket": f"d={ms}"},
    "RelyingParty": "http://auth.xboxlive.com", "TokenType": "JWT"})
assert s == 200, (s, xbl)
uhs_xbl = xbl["DisplayClaims"]["xui"][0]["uhs"]
log(f"[3] XBL ok: uhs={uhs_xbl[:6]}… token len={len(xbl['Token'])}")

def xsts(rp):
    return post_json("https://xsts.auth.xboxlive.com/xsts/authorize", {
        "Properties": {"SandboxId": "RETAIL", "UserTokens": [xbl["Token"]]},
        "RelyingParty": rp, "TokenType": "JWT"})

# 4. XSTS (документированный RP)
s, x, _ = xsts("rp://api.minecraftservices.com/")
assert s == 200, (s, x)
uhs = x["DisplayClaims"]["xui"][0]["uhs"]
tok_mc = x["Token"]
log(f"[4] XSTS ok (RP с /): uhs={uhs[:6]}… (XBL uhs совпал: {uhs == uhs_xbl}) token len={len(tok_mc)}")

def mc_login(uhs, xtok, label):
    s, b, h = post_json("https://api.minecraftservices.com/authentication/login_with_xbox",
                        {"identityToken": f"XBL3.0 x={uhs};{xtok}"})
    log(f"[5] loginWithXbox [{label}]: HTTP {s} body={json.dumps(b, ensure_ascii=False)[:200] if not isinstance(b, str) else b[:200]}")
    return s

r1 = mc_login(uhs, tok_mc, "эталон")
if r1 == 200: sys.exit(0)

# варианты
s2, x2, _ = xsts("rp://api.minecraftservices.com")     # без слэша
if s2 == 200:
    mc_login(x2["DisplayClaims"]["xui"][0]["uhs"], x2["Token"], "RP без слэша")
mc_login(uhs_xbl, tok_mc, "uhs из XBL")
mc_login(uhs, tok_mc, "повтор того же токена")          # одноразовость?
