"""Checks bugsink-rs's login, sessions, CSRF and access rules against what upstream allows (accounts.sh).

Usage: accounts_api.py <base url> <expect.json> <state.json> before|after
  before: everything, ending logged in as the member (session kept in state.json)
  after:  upstream changed the member's password meanwhile: that session is over, the new password works
"""
import json
import sys
import urllib.error
import urllib.request

base, expect_path, state_path, phase = sys.argv[1:]
expect = json.load(open(expect_path))
failures = 0


def check(what, ok, detail=""):
    global failures
    print(("ok   " if ok else "FAIL ") + what + ("" if ok else f": {detail}"))
    failures += not ok


def call(method, path, body=None, cookie=None, csrf=None, origin=None, content_type="application/json"):
    data = None if body is None else (json.dumps(body) if content_type == "application/json" else body).encode()
    req = urllib.request.Request(base + path, data=data, method=method)
    if data is not None:
        req.add_header("Content-Type", content_type)
    if cookie:
        req.add_header("Cookie", cookie)
    if csrf:
        req.add_header("X-CSRFToken", csrf)
    if origin:
        req.add_header("Origin", origin)
    try:
        resp = urllib.request.urlopen(req)
    except urllib.error.HTTPError as e:
        resp = e
    raw = resp.read()
    try:
        payload = json.loads(raw) if raw else None
    except ValueError:
        payload = raw.decode(errors="replace")
    return resp.status, payload, resp.headers.get_all("Set-Cookie") or []


def login(username, password, **kw):
    status, me, cookies = call("POST", "/api/v1/auth/login", {"username": username, "password": password}, **kw)
    cookie = next((c.split(";")[0] for c in cookies if c.startswith("bsrs_sessionid=")), None)
    return status, me, cookie


if phase == "before":
    status, _, _ = call("GET", "/api/v1/projects")
    check("no session: 401", status == 401, status)
    status, _, _ = call("GET", "/api/v1/projects", cookie="bsrs_sessionid=" + "0" * 64)
    check("unknown session: 401", status == 401, status)

    status, body, cookie = login("member@example.org", "wrong")
    check("wrong password: 400, no cookie", status == 400 and cookie is None, (status, body))
    status, body2, _ = login("nobody@example.org", "member-pw")
    check("unknown user: same answer", status == 400 and body2 == body, (status, body2))
    status, _, _ = login("inactive@example.org", "inactive-pw")
    check("inactive user: 400", status == 400, status)
    status, _, _ = login("Member@example.org", "member-pw")
    check("username is case-sensitive", status == 400, status)
    status, _, _ = login("member@example.org", "member-pw", origin="https://evil.example")
    check("login from another origin: 403", status == 403, status)
    status, _, _ = call("POST", "/api/v1/auth/login", "username=member%40example.org&password=member-pw",
                        content_type="application/x-www-form-urlencoded")
    check("login as a form post: refused", status == 415, status)

    for username, exp in expect.items():
        status, me, cookie = login(username, exp["password"], origin=base)
        check(f"{username}: login", status == 200 and cookie and me["username"] == username, (status, me))
        if status != 200:
            continue
        csrf = me["csrf_token"]
        status, me2, _ = call("GET", "/api/v1/me", cookie=cookie)
        check(f"{username}: /me", status == 200 and me2 == me, (status, me2))
        status, projects, _ = call("GET", "/api/v1/projects", cookie=cookie)
        names = sorted(p["name"] for p in projects) if status == 200 else projects
        check(f"{username}: visible projects {exp['visible']}", names == exp["visible"], names)
        for name, (project_id, allowed) in exp["access"].items():
            status, _, _ = call("GET", f"/api/v1/projects/{project_id}/issues", cookie=cookie)
            check(f"{username}: issues of {name} {'200' if allowed else '403'}", status == (200 if allowed else 403),
                  status)
        status, _, _ = call("GET", "/api/v1/projects/999999/issues", cookie=cookie)
        check(f"{username}: issues of a missing project 404", status == 404, status)
        status, _, _ = call("GET", "/api/v1/issues/00000000-0000-0000-0000-000000000000", cookie=cookie)
        check(f"{username}: missing issue 404", status == 404, status)

        if username != "member@example.org":
            status, _, _ = call("POST", "/api/v1/auth/logout", cookie=cookie)
            check(f"{username}: logout without CSRF token: 403", status == 403, status)
            status, _, _ = call("POST", "/api/v1/auth/logout", cookie=cookie, csrf="x" * 64)
            check(f"{username}: logout with a wrong token: 403", status == 403, status)
            status, _, _ = call("POST", "/api/v1/auth/logout", cookie=cookie, csrf=csrf, origin="https://evil.example")
            check(f"{username}: logout from another origin: 403", status == 403, status)
            status, _, cookies = call("POST", "/api/v1/auth/logout", cookie=cookie, csrf=csrf, origin=base)
            check(f"{username}: logout", status == 204 and any("Max-Age=0" in c for c in cookies), (status, cookies))
            status, _, _ = call("GET", "/api/v1/me", cookie=cookie)
            check(f"{username}: session over after logout", status == 401, status)
        else:
            json.dump({"cookie": cookie}, open(state_path, "w"))

elif phase == "after":
    cookie = json.load(open(state_path))["cookie"]
    status, _, _ = call("GET", "/api/v1/me", cookie=cookie)
    check("password changed upstream: session over", status == 401, status)
    status, _, _ = login("member@example.org", "member-pw")
    check("old password refused", status == 400, status)
    status, _, cookie = login("member@example.org", "changed-pw")
    check("new password (set upstream) works", status == 200 and cookie, status)

sys.exit(1 if failures else 0)
