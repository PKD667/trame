#!/usr/bin/env python3
"""Rust-analyzer's errors over the tree, as an editor sees them: the server is started on ROOT, reads ROOT/rust-analyzer.toml,
and is given the `rust-analyzer.*` settings of ROOT/.vscode/settings.json as an editor would; it is then asked for the
diagnostics of every .rs file of the workspace. Prints one line per error, then the count; exits 1
if there is one. The `rust-analyzer diagnostics` command is not a substitute: it ignores rust-analyzer.toml.
  python3 check/editor/diagnostics.py ROOT        (rust-analyzer on PATH; in a dev shell, for the toolchain)
"""
import json, os, subprocess, sys, time

root = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else ".")
skip = {"target", ".git", "result", "driver", "rejections"}
# Submodules are other repositories: their paths are the `path = ...` lines of .gitmodules.
modules = set()
if os.path.exists(os.path.join(root, ".gitmodules")):
    for line in open(os.path.join(root, ".gitmodules")):
        if line.strip().startswith("path"):
            modules.add(os.path.join(root, line.split("=", 1)[1].strip()))
files = []
for d, ds, fs in os.walk(root):
    ds[:] = [x for x in ds if x not in skip and os.path.join(d, x) not in modules]
    files += [os.path.join(d, f) for f in fs if f.endswith(".rs")]
files.sort()

# The editor's settings: "rust-analyzer.cargo.cfgs": [...] becomes {"cargo": {"cfgs": [...]}}.
options = {}
settings = os.path.join(root, ".vscode", "settings.json")
if os.path.exists(settings):
    for key, value in json.load(open(settings)).items():
        if key.startswith("rust-analyzer."):
            *path, leaf = key[len("rust-analyzer."):].split(".")
            node = options
            for k in path: node = node.setdefault(k, {})
            node[leaf] = value

p = subprocess.Popen(["rust-analyzer"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
nid = 0
def send(m):
    b = json.dumps(m).encode()
    p.stdin.write(b"Content-Length: %d\r\n\r\n" % len(b) + b); p.stdin.flush()
def read():
    n = 0
    while True:
        l = p.stdout.readline()
        if not l: return None
        l = l.strip()
        if not l: break
        if l.lower().startswith(b"content-length:"): n = int(l.split(b":")[1])
    return json.loads(p.stdout.read(n))
def request(method, params):
    global nid; nid += 1; me = nid
    send({"jsonrpc": "2.0", "id": me, "method": method, "params": params})
    while True:
        m = read()
        if m is None: sys.exit("server ended")
        if "id" in m and "method" in m:  # a request of the server: refuse politely
            send({"jsonrpc": "2.0", "id": m["id"], "result": None}); continue
        if m.get("id") == me: return m

uri = lambda f: "file://" + f
send({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
    "processId": os.getpid(), "rootUri": uri(root),
    "initializationOptions": options,
    "workspaceFolders": [{"uri": uri(root), "name": "trame"}],
    "capabilities": {"experimental": {"serverStatusNotification": True},
                     "textDocument": {"diagnostic": {"dynamicRegistration": False}}}}})
while (m := read()) and m.get("id") != 0: pass
send({"jsonrpc": "2.0", "method": "initialized", "params": {}})

# Wait for the workspace to load and the build scripts to run: the server says it is quiescent.
t0 = time.time()
while True:
    m = read()
    if m is None: sys.exit("server ended")
    if "id" in m and "method" in m: send({"jsonrpc": "2.0", "id": m["id"], "result": None})
    if m.get("method") == "experimental/serverStatus" and m["params"].get("quiescent"):
        if m["params"].get("health") == "error": print("server health: error:", m["params"].get("message"))
        break
    if time.time() - t0 > 900: sys.exit("timeout waiting for the workspace")

errors = 0
for f in files:
    r = request("textDocument/diagnostic", {"textDocument": {"uri": uri(f)}})
    for d in (r.get("result") or {}).get("items", []):
        if d.get("severity") == 1:
            errors += 1
            s = d["range"]["start"]
            print(f"{os.path.relpath(f, root)}:{s['line']+1}:{s['character']+1}: {d.get('code')}: {d['message'].splitlines()[0]}")
print(f"{errors} errors in {len(files)} files")
request("shutdown", None); send({"jsonrpc": "2.0", "method": "exit"})
sys.exit(1 if errors else 0)
