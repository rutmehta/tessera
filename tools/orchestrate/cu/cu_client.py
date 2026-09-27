#!/usr/bin/env python3
"""Minimal codex app-server client: runs one turn, auto-answers Computer Use
app-approval elicitations for an allowlist of bundle ids, declines everything else."""
import json, subprocess, sys, os, threading
ALLOW = set(os.environ.get("CU_ALLOW", "com.apple.finder").split(","))
MODEL = os.environ.get("CU_MODEL", "gpt-6-sol")
CWD = os.environ.get("CU_CWD", os.getcwd())
prompt = sys.argv[1]
p = subprocess.Popen(["codex", "app-server"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                     stderr=open(os.devnull, "w"), text=True, bufsize=1)
nid = [0]
def send(obj): p.stdin.write(json.dumps(obj) + "\n"); p.stdin.flush()
def req(method, params):
    nid[0] += 1; send({"jsonrpc": "2.0", "id": nid[0], "method": method, "params": params}); return nid[0]
pending = {}
def call(method, params):
    i = req(method, params)
    while True:
        m = handle(json.loads(p.stdout.readline()))
        if m is not None and m.get("id") == i and ("result" in m or "error" in m): return m
done = [False]
def handle(m):
    meth = m.get("method")
    if meth and "id" in m:  # server -> client request
        if meth == "mcpServer/elicitation/request":
            pr = m["params"]; meta = pr.get("_meta") or {}
            app = ((meta.get("tool_params") or {}).get("app"))
            ok = pr.get("serverName") == "cua_repl" and app in ALLOW
            print(f"[elicitation] server={pr.get('serverName')} app={app} msg={pr.get('message')!r} -> {'accept' if ok else 'decline'}", flush=True)
            send({"jsonrpc": "2.0", "id": m["id"], "result": {"action": "accept" if ok else "decline", "content": {} if ok else None, "_meta": {"persist": "session"} if ok else None}})
        else:
            print(f"[server request {meth}] -> decline", flush=True)
            send({"jsonrpc": "2.0", "id": m["id"], "result": {"decision": "decline"}})
        return None
    if meth == "item/completed":
        it = m["params"].get("item", {})
        if it.get("type") == "agentMessage": print("[agent]", it.get("text"), flush=True)
        elif it.get("type") == "mcpToolCall": print("[tool]", it.get("server"), it.get("tool"), it.get("status"), json.dumps(it.get("result") or it.get("error"))[:1500], flush=True)
    if meth == "turn/completed": done[0] = True
    return m
call("initialize", {"clientInfo": {"name": "cu_client", "version": "0.1"}, "capabilities": {"experimentalApi": True, "requestAttestation": False}})
send({"jsonrpc": "2.0", "method": "initialized"})
r = call("thread/start", {"model": MODEL, "cwd": CWD, "sandbox": "read-only", "ephemeral": True,
     "approvalPolicy": {"granular": {"sandbox_approval": False, "rules": False, "skill_approval": False, "request_permissions": False, "mcp_elicitations": True}}})
if "error" in r: print(r); sys.exit(1)
tid = r["result"]["thread"]["id"]
call("turn/start", {"threadId": tid, "input": [{"type": "text", "text": prompt, "text_elements": []}]})
while not done[0]:
    line = p.stdout.readline()
    if not line: break
    handle(json.loads(line))
p.terminate()
