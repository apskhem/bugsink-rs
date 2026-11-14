"""rest_api.sh's driver: sends the steps (rest-steps.jsonl, on stdin) to one side's REST API, printing one JSON line per
step with what came back (status, the headers that matter, the body as text).

Usage: rest_api.py <base url> <postgres container> <database> < rest-steps.jsonl"""
import http.client
import json
import os
import re
import subprocess
import sys
import urllib.parse

sys.path.insert(0, os.path.dirname(__file__))
from make_rest_seed import TOKENS  # noqa: E402

base, pg, db = sys.argv[1:]
url = urllib.parse.urlsplit(base)
responses = {}


def psql(sql):
    out = subprocess.run(["docker", "exec", pg, "psql", "-U", "postgres", "-d", db, "-At", "-c", sql],
                         stdin=subprocess.DEVNULL, capture_output=True, text=True, check=True).stdout
    return out.strip()


def resolve(text):
    def one(m):
        kind, arg = m.group(1), m.group(2)
        if kind == "created":
            return str(json.loads(responses[int(arg)]["body"])["id"])
        if kind in ("next", "previous"):
            link = json.loads(responses[int(arg)]["body"])[kind] or "/api/canonical/0/no-link/"
            parts = urllib.parse.urlsplit(link)
            return parts.path + ("?" + parts.query if parts.query else "")
        if kind == "issue":
            return psql("SELECT i.id FROM issues_issue i JOIN issues_grouping g ON g.issue_id = i.id "
                        f"WHERE g.grouping_key = '{arg}' LIMIT 1") or "00000000-0000-0000-0000-000000000000"
        if kind == "eventpk":
            found = psql(f"SELECT id FROM events_event WHERE event_id = '{arg}'")
            return found or "00000000-0000-0000-0000-000000000000"
        raise ValueError(kind)
    return re.sub(r"\{(created|next|previous|issue|eventpk):([^}]+)\}", one, text)


for line in sys.stdin:
    step = json.loads(line)
    path = resolve(step["path"])
    headers = {"Host": "bugsink.test"}
    if step.get("token"):
        headers["Authorization"] = f"Bearer {TOKENS[step['token']]}"
    body = None
    if "body" in step:
        body = resolve(json.dumps(step["body"], ensure_ascii=False)).encode()
        headers["Content-Type"] = "application/json"
    elif "raw" in step:
        body = step["raw"].encode()
        headers["Content-Type"] = "application/json"
    for k, v in (step.get("headers") or {}).items():
        if k == "Content-Type" and v == "":
            headers.pop(k, None)
        else:
            headers[k] = v
    conn = http.client.HTTPConnection(url.hostname, url.port, timeout=60)
    conn.putrequest(step["method"], path, skip_host=True, skip_accept_encoding=True)
    for k, v in headers.items():
        conn.putheader(k, v)
    if body is not None:
        conn.putheader("Content-Length", str(len(body)))
    conn.endheaders(body)
    r = conn.getresponse()
    text = r.read().decode("utf-8", "replace")
    conn.close()
    result = {"n": step["n"], "status": r.status, "content_type": r.getheader("Content-Type"),
              "allow": r.getheader("Allow"), "www_authenticate": r.getheader("WWW-Authenticate"), "body": text}
    responses[step["n"]] = result
    print("STEP " + json.dumps(result, ensure_ascii=False), flush=True)
