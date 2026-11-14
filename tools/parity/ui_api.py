"""Checks bugsink-rs's UI API (ui_api.sh): login, sessions, CSRF and access rules against what upstream allows, then
issue lists and issue actions on events sent to our own ingest.

Usage: ui_api.py <base url> <expect.json> <state.json> before|after
  before: everything, ending logged in as the member (session kept in state.json)
  after:  upstream changed the member's password meanwhile: that session is over, the new password works
"""
import json
import sys
import time
import urllib.error
import urllib.request
import uuid

base, expect_path, state_path, phase = sys.argv[1:]
expect_all = json.load(open(expect_path))
expect, sentry_keys = expect_all["users"], expect_all["sentry_keys"]
failures = 0


def check(what, ok, detail=""):
    global failures
    print(("ok   " if ok else "FAIL ") + what + ("" if ok else f": {detail}"))
    failures += not ok


def call(method, path, body=None, cookie=None, csrf=None, origin=None, content_type="application/json", headers=()):
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
    for k, v in headers:
        req.add_header(k, v)
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
            member = (cookie, csrf)
            json.dump({"cookie": cookie}, open(state_path, "w"))

    # Issues, from events sent to our own ingest: "b" twice, so it sorts first by event count.
    mine = expect["member@example.org"]["access"]["Mine"][0]
    auth = ("X-Sentry-Auth", f"Sentry sentry_version=7, sentry_key={sentry_keys['Mine']}")
    # ui_api.sh runs the server behind one proxy that sets X-Forwarded-For; only "a" says where it came from.
    status, _, _ = call("POST", f"/api/{mine}/store/", {"message": "spoofed"},
                        headers=[auth, ("X-Forwarded-For", "6.6.6.6, 198.51.100.7")])
    check("ingest with a spoofed X-Forwarded-For: 400", status == 400, status)
    for message, tags in [("a", {}), ("b", {"region": "eu"}), ("b", {"region": "us"}), ("c", {"region": "eu"})]:
        forwarded = [("X-Forwarded-For", "198.51.100.7")] if message == "a" else []
        status, body, _ = call("POST", f"/api/{mine}/store/",
                               {"event_id": uuid.uuid4().hex, "message": message, "tags": tags},
                               headers=[auth] + forwarded)
        check(f"ingest {message!r}", status == 200, (status, body))
    cookie, csrf = member

    def issues(path, **params):
        query = "&".join(f"{k}={v}" for k, v in params.items())
        status, page, _ = call("GET", f"{path}?{query}", cookie=cookie)
        return {i["calculated_value"]: i for i in page["items"]} if status == 200 else status

    def act(issue, action, **kw):
        return call("POST", f"/api/v1/issues/{issue}/actions", {"action": action}, **kw)

    def bulk(path, action, ids, **kw):
        return call("POST", path, {"action": action, "issue_ids": ids}, **kw)

    project_issues = f"/api/v1/projects/{mine}/issues"
    for _ in range(100):  # digest runs in the background
        listed = issues(project_issues, state="all")
        if isinstance(listed, dict) and sum(i["digested_event_count"] for i in listed.values()) == 4:
            break
        time.sleep(0.1)
    check("digested into 3 issues", sorted(listed) == ["a", "b", "c"], listed)
    ids = {value: issue["id"] for value, issue in listed.items()}
    status, page, _ = call("GET", f"{project_issues}?state=all&sort=events", cookie=cookie)
    check("sort=events", status == 200 and page["items"][0]["calculated_value"] == "b", page)
    status, _, _ = call("GET", f"{project_issues}?state=bogus", cookie=cookie)
    check("unknown tab: 400", status == 400, status)
    check("search: tag", sorted(issues(project_issues, state="all", q="region:eu")) == ["b", "c"], None)
    check("search: tag and text", sorted(issues(project_issues, state="all", q="region:eu%20B")) == ["b"], None)
    check("search: unknown tag value", issues(project_issues, state="all", q="region:mars") == {}, None)
    status, page, _ = call("GET", f"/api/v1/issues/{ids['b']}/events?q=region:us", cookie=cookie)
    check("search: events", status == 200 and len(page["items"]) == 1, (status, page))
    status, _, _ = call("GET", "/api/v1/issues?q=region:eu", cookie=cookie)
    check("search on the global list: 400", status == 400, status)

    status, result, _ = act(ids["a"], "resolve", cookie=cookie, csrf=csrf)
    check("resolve", status == 200 and result["applied"] and result["issue"]["is_resolved"], (status, result))
    status, result, _ = act(ids["a"], "resolve", cookie=cookie, csrf=csrf)
    check("resolve again: not applied", status == 200 and not result["applied"], (status, result))
    status, result, _ = act(ids["a"], "mute", cookie=cookie, csrf=csrf)
    check("mute a resolved issue: not applied", status == 200 and not result["applied"], (status, result))
    check("open tab", sorted(issues(project_issues)) == ["b", "c"], issues(project_issues))
    check("resolved tab", sorted(issues(project_issues, state="resolved")) == ["a"], None)
    status, _, _ = act(ids["b"], "frobnicate", cookie=cookie, csrf=csrf)
    check("unknown action: 400", status == 400, status)
    status, _, _ = act(ids["b"], "mute", cookie=cookie)
    check("action without CSRF token: 403", status == 403, status)

    status, result, _ = bulk(f"{project_issues}/actions", "mute_for:day,1,None", list(ids.values()), cookie=cookie,
                             csrf=csrf)
    check("bulk mute: the resolved one unapplied", status == 200 and result["unapplied_issue_ids"] == [ids["a"]],
          (status, result))
    check("muted tab", sorted(issues(project_issues, state="muted")) == ["b", "c"], None)
    status, result, _ = bulk(f"{project_issues}/actions", "unmute", [ids["b"]], cookie=cookie, csrf=csrf)
    check("bulk unmute", status == 200 and result["unapplied_issue_ids"] == [], (status, result))

    status, _, outsider = login("outsider@example.org", "outsider-pw")
    status, me, _ = call("GET", "/api/v1/me", cookie=outsider)
    outsider_csrf = me["csrf_token"]
    status, _, _ = act(ids["c"], "unmute", cookie=outsider, csrf=outsider_csrf)
    check("outsider: action 403", status == 403, status)
    status, _, _ = bulk(f"{project_issues}/actions", "unmute", [ids["c"]], cookie=outsider, csrf=outsider_csrf)
    check("outsider: project bulk action 403", status == 403, status)
    status, result, _ = bulk("/api/v1/issues/actions", "unmute", [ids["c"]], cookie=outsider, csrf=outsider_csrf)
    check("outsider: global bulk action selects nothing", status == 200 and result["unapplied_issue_ids"] == [],
          (status, result))
    check("... and changes nothing", sorted(issues(project_issues, state="muted")) == ["c"], None)
    status, page, _ = call("GET", "/api/v1/issues?state=all", cookie=outsider)
    check("outsider: global list empty", status == 200 and page["items"] == [], (status, page))

    check("member: global list", sorted(issues("/api/v1/issues", state="all")) == ["a", "b", "c"], None)
    status, _, _ = bulk("/api/v1/issues/actions", "resolved_next", [ids["b"]], cookie=cookie, csrf=csrf)
    check("global bulk resolved_next: 403", status == 403, status)
    status, result, _ = bulk("/api/v1/issues/actions", "unmute", [ids["b"], ids["c"]], cookie=cookie, csrf=csrf)
    check("global bulk unmute", status == 200 and result["unapplied_issue_ids"] == [ids["b"]], (status, result))

    # Issue pages ("b": two events, region eu then us)
    status, page, _ = call("GET", f"/api/v1/issues/{ids['b']}", cookie=cookie)
    check("issue page", status == 200 and page["friendly_id"] == "MINE-2" and page["title"] == "b", (status, page))
    status, tags, _ = call("GET", f"/api/v1/issues/{ids['b']}/tags", cookie=cookie)
    region = next((k for k in tags if k["key"] == "region"), None) if status == 200 else None
    check("tags", region is not None and sorted((v["value"], v["count"], v["pct"]) for v in region["values"])
          == [("eu", 1, 50), ("us", 1, 50)], (status, tags))
    status, groupings, _ = call("GET", f"/api/v1/issues/{ids['b']}/groupings", cookie=cookie)
    check("groupings", status == 200 and len(groupings) == 1, (status, groupings))
    status, first, _ = call("GET", f"/api/v1/issues/{ids['b']}/events/first", cookie=cookie)
    check("first event", status == 200 and first["nav"] == {"first": 1, "prev": None, "next": 2, "last": 2, "count": 2},
          (status, first))
    display = first.get("display") or {} if status == 200 else {}
    key_info = dict(display.get("key_info", []))
    check("event display", key_info.get("digest order") == 1 and key_info.get("title") == "b"
          and display["deployment_info"] == [] and display["stacktrace"]["stack_of_plates"], display)
    status, nxt, _ = call("GET", f"/api/v1/issues/{ids['b']}/events/1/next", cookie=cookie)
    check("next event", status == 200 and nxt["event"]["digest_order"] == 2, (status, nxt))
    status, _, _ = call("GET", f"/api/v1/issues/{ids['b']}/events/2/next", cookie=cookie)
    check("no next event: 404", status == 404, status)
    status, last, _ = call("GET", f"/api/v1/issues/{ids['b']}/events/last?q=region:eu", cookie=cookie)
    check("last event in a search", status == 200 and last["event"]["digest_order"] == 1
          and last["nav"]["count"] == 1, (status, last))
    status, by_id, _ = call("GET", f"/api/v1/issues/{ids['b']}/events/{nxt['event']['event_id']}", cookie=cookie)
    check("event by the SDK's event_id", status == 200 and by_id["event"]["digest_order"] == 2, status)
    status, counts, _ = call("GET", f"/api/v1/issues/{ids['b']}/counts", cookie=cookie)
    check("hourly counts", status == 200 and sum(h["count"] for h in counts["hours"]) == 2, (status, counts))
    status, page, _ = call("GET", f"{project_issues}?state=all", cookie=cookie)
    check("issue list sparklines", status == 200 and all(len(i["sparkline"]) == 24 for i in page["items"])
          and sum(i["sparkline"][-1] for i in page["items"]) == 4, page)
    status, projects, _ = call("GET", "/api/v1/projects", cookie=cookie)
    mine_row = next(p for p in projects if p["id"] == mine)
    check("project list sparkline", mine_row["sparkline"][-1] == 4, mine_row)

    history = f"/api/v1/issues/{ids['b']}/history"
    status, body, _ = call("POST", f"/api/v1/issues/{ids['b']}/comments", {"comment": "  looking into it \n"},
                           cookie=cookie, csrf=csrf)
    check("comment", status == 201 and body["id"], (status, body))
    comment_id = body["id"]
    status, body, _ = call("POST", f"/api/v1/issues/{ids['b']}/comments", {"comment": "   "}, cookie=cookie, csrf=csrf)
    check("empty comment ignored", status == 200 and body["id"] is None, (status, body))
    status, entries, _ = call("GET", history, cookie=cookie)
    check("history: newest first, comment stripped", status == 200 and entries[0]["kind"] == 100
          and entries[0]["comment"] == "looking into it" and entries[0]["user"]["display_name"] == "member@example.org",
          (status, entries[:1]))
    comment = f"/api/v1/issues/{ids['b']}/comments/{comment_id}"
    status, _, _ = call("PATCH", comment, {"comment": "fixed by #12"}, cookie=cookie, csrf=csrf)
    check("edit own comment", status == 204, status)
    # the superuser sees the issue, but the comment isn't theirs
    status, admin_me, admin = login("admin@Example.COM", "admin-pw")
    status, _, _ = call("PATCH", comment, {"comment": "x"}, cookie=admin, csrf=admin_me["csrf_token"])
    check("someone else's comment: edit 403", status == 403, status)
    status, _, _ = call("DELETE", comment, cookie=admin, csrf=admin_me["csrf_token"])
    check("someone else's comment: delete 403", status == 403, status)
    resolved_by_member = next(e for e in call("GET", f"/api/v1/issues/{ids['a']}/history", cookie=cookie)[1]
                              if e["kind"] == 2)
    status, _, _ = call("DELETE", f"/api/v1/issues/{ids['a']}/comments/{resolved_by_member['id']}", cookie=cookie,
                        csrf=csrf)
    check("delete an action's turning point: 403", status == 403, status)
    status, _, _ = call("PATCH", f"/api/v1/issues/{ids['a']}/comments/{resolved_by_member['id']}",
                        {"comment": "was a config issue"}, cookie=cookie, csrf=csrf)
    check("comment on own action", status == 204, status)
    status, _, _ = call("DELETE", f"/api/v1/issues/{ids['a']}/comments/{comment_id}", cookie=cookie, csrf=csrf)
    check("comment of another issue: 404", status == 404, status)
    status, _, _ = call("DELETE", comment, cookie=cookie, csrf=csrf)
    check("delete own comment", status == 204, status)
    status, entries, _ = call("GET", history, cookie=cookie)
    check("comment gone", all(e["id"] != comment_id for e in entries), entries[:1])

    status, result, _ = act(ids["c"], "delete", cookie=cookie, csrf=csrf)
    check("delete", status == 200 and result["applied"] and result["issue"] is None, (status, result))
    status, _, _ = call("GET", f"/api/v1/issues/{ids['c']}", cookie=cookie)
    check("deleted issue: 404", status == 404, status)
    check("deleted issue not listed", sorted(issues(project_issues, state="all")) == ["a", "b"], None)

elif phase == "after":
    cookie = json.load(open(state_path))["cookie"]
    status, _, _ = call("GET", "/api/v1/me", cookie=cookie)
    check("password changed upstream: session over", status == 401, status)
    status, _, _ = login("member@example.org", "member-pw")
    check("old password refused", status == 400, status)
    status, _, cookie = login("member@example.org", "changed-pw")
    check("new password (set upstream) works", status == 200 and cookie, status)

sys.exit(1 if failures else 0)
