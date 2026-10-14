"""Points the fixed address at a quick tunnel, for the lower ping.

A free-plan zone's addresses land at whatever Cloudflare colo has room (often one far from the
player, ping about 100 ms), while a trycloudflare.com quick tunnel lands at the nearest one
(about half that). So the running server gets a second, quick tunnel next to its named one, and
the zone's page (`https://<hostname>/`, path "/" only) redirects there with a 302. The launcher's
Join, `/api/*` and the sockets on the fixed address keep going through the named tunnel.

    python tools/fast_link.py          start the quick tunnel if none answers, then sync the rule
    python tools/fast_link.py --sync   only point the rule at the quick tunnel that is running

The redirect needs a Cloudflare API token with Zone, Single Redirect, Edit for the zone, saved as
the only line of `%APPDATA%\\mc-rs\\server\\cloudflare-token.txt`. Without it the quick tunnel still
starts and its link is printed. Nothing restarts the server.
"""

import json
import os
import re
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

DATA = Path(os.environ["APPDATA"]) / "mc-rs" / "server"
LOG = DATA / "quick-tunnel.log"
TOKEN = DATA / "cloudflare-token.txt"
API = "https://api.cloudflare.com/client/v4"
URL_RE = re.compile(r"https://[a-z0-9-]+\.trycloudflare\.com")


def config():
    cfg = json.loads((DATA / "server.json").read_text(encoding="utf-8"))
    return cfg.get("hostname", ""), cfg.get("port", 25580)


def logged_url():
    if not LOG.exists():
        return None
    found = URL_RE.findall(LOG.read_text(encoding="utf-8", errors="replace"))
    return found[-1] if found else None


def answers(url):
    try:
        with urllib.request.urlopen(url + "/api/ping", timeout=10) as r:
            return r.status == 200
    except Exception:
        return False


def start_tunnel(port):
    """cloudflared on its own, outside this shell's job, so it outlives whatever started it."""
    LOG.write_text("", encoding="utf-8")
    flags = 0x00000008 | 0x01000000 | 0x08000000  # DETACHED_PROCESS, BREAKAWAY_FROM_JOB, NO_WINDOW
    subprocess.Popen(
        ["cloudflared", "tunnel", "--no-autoupdate", "--logfile", str(LOG), "--url", f"http://127.0.0.1:{port}"],
        creationflags=flags,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        close_fds=True,
    )
    for _ in range(60):
        time.sleep(1)
        url = logged_url()
        if url and answers(url):
            return url
    sys.exit(f"the quick tunnel did not come up in 60 s; see {LOG}")


def api(token, method, path, body=None):
    req = urllib.request.Request(
        API + path,
        method=method,
        data=None if body is None else json.dumps(body).encode(),
        headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=20) as r:
            return json.load(r)
    except urllib.error.HTTPError as e:
        sys.exit(f"Cloudflare said {e.code} on {method} {path}: {e.read().decode(errors='replace')[:400]}")


def rule_for(hostname, url):
    return {
        "description": "mc-rs: page to the quick tunnel (tools/fast_link.py)",
        "expression": f'(http.host eq "{hostname}" and http.request.uri.path eq "/")',
        "action": "redirect",
        "action_parameters": {
            "from_value": {"status_code": 302, "target_url": {"value": url + "/"}, "preserve_query_string": True}
        },
    }


def sync(hostname, url):
    if not TOKEN.exists():
        print(f"no {TOKEN}: the redirect was left as it is")
        return
    token = TOKEN.read_text(encoding="utf-8").strip()
    zones = api(token, "GET", f"/zones?name={hostname}")["result"]
    if not zones:
        sys.exit(f"the token sees no zone named {hostname}")
    zone = zones[0]["id"]
    # PUT on the phase entry point replaces its rules, and creates it on the first run.
    api(token, "PUT", f"/zones/{zone}/rulesets/phases/http_request_dynamic_redirect/entrypoint",
        {"rules": [rule_for(hostname, url)]})
    print(f"https://{hostname}/ now redirects to {url}/")


def main():
    hostname, port = config()
    url = logged_url()
    if not (url and answers(url)):
        if "--sync" in sys.argv:
            sys.exit("no quick tunnel answers; run without --sync to start one")
        url = start_tunnel(port)
    print(f"quick tunnel: {url}")
    if hostname:
        sync(hostname, url)


if __name__ == "__main__":
    main()
