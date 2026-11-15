"""admin.sh's driver for bugsink-rs: the steps (JSONL on stdin) through our UI API, printing the same JSON lines as
upstream_admin.py (outcome, form errors, invite link, what a list showed, the database snapshot afterwards).

Usage: admin_api.py <base url> <postgres container> <database> <mode> < steps.jsonl"""
import json
import subprocess
import sys
import time
import urllib.error
import urllib.request

base, pg, db, mode, capture = sys.argv[1:]
PASSWORDS = {f"{name}@example.org": f"pw-{name}" for name in ["alice", "bob", "carol", "admin"]}
sessions = {}  # email -> (cookie, csrf token)


def psql(sql):
    out = subprocess.run(["docker", "exec", pg, "psql", "-U", "postgres", "-d", db, "-At", "-c", sql],
                         stdin=subprocess.DEVNULL, capture_output=True, text=True, check=True).stdout
    return [line for line in out.splitlines() if line]


def snapshot():
    out = subprocess.run(["docker", "exec", "-i", pg, "psql", "-U", "postgres", "-d", db, "-At", "-f", "-"],
                         input=open("tools/parity/admin_snapshot.sql").read(), capture_output=True, text=True,
                         check=True).stdout
    return [line for line in out.splitlines() if line]


def wait_for_jobs():
    for _ in range(100):
        if psql("SELECT count(*) FROM bsrs_job WHERE failed_at IS NULL") == ["0"]:
            return
        time.sleep(0.1)
    raise RuntimeError("jobs didn't finish")


def call(method, path, body=None, session=None):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(base + path, data=data, method=method)
    req.add_header("Content-Type", "application/json")
    req.add_header("Origin", base)
    if session:
        req.add_header("Cookie", session[0])
        req.add_header("X-CSRFToken", session[1])
    try:
        resp = urllib.request.urlopen(req)
    except urllib.error.HTTPError as e:
        resp = e
    raw = resp.read()
    cookie = next((c.split(";")[0] for c in resp.headers.get_all("Set-Cookie") or [] if c.startswith("bsrs_session")),
                  None)
    return resp.status, json.loads(raw) if raw else None, cookie


def session(email):
    if email not in sessions:
        status, me, cookie = call("POST", "/api/v1/auth/login", {"username": email, "password": PASSWORDS[email]})
        assert status == 200, (email, status, me)
        sessions[email] = (cookie, me["csrf_token"])
    return sessions[email]


def one(sql):
    rows = psql(sql)
    return rows[0] if rows else None


def team_id(name):
    return one(f"SELECT id FROM teams_team WHERE name = '{name}'") or "00000000-0000-0000-0000-000000000000"


def project_id(slug):
    return one(f"SELECT id FROM projects_project WHERE slug = '{slug}'") or "999999"


def user_id(email):
    return one(f"SELECT id FROM auth_user WHERE email = '{email}'")


def token_for(email):
    return one(f"SELECT v.token FROM users_emailverification v JOIN auth_user u ON u.id = v.user_id "
               f"WHERE u.email = '{email}' ORDER BY v.id DESC LIMIT 1") or "missing-token"


def run(s):
    op = s["op"]
    kind = "teams" if op.startswith("team") else "projects"
    group = lambda: (f"/api/v1/teams/{team_id(s['team'])}" if kind == "teams"  # noqa
                     else f"/api/v1/projects/{project_id(s['project'])}")
    anonymous = ("accept_new_user", "reset_password", "signup", "confirm_email", "resend_confirmation",
                 "request_reset_password")
    me = None if op in anonymous else session(s["as"])
    fields = lambda *keys: {k: s[k] for k in keys if k in s}  # noqa
    project_fields = ("name", "visibility", "retention_max_event_count", "grouping_mechanism")
    calls = {
        "team_create": lambda: ("POST", "/api/v1/teams", fields("name", "visibility")),
        "team_edit": lambda: ("PATCH", group(), fields("name", "visibility")),
        "team_delete": lambda: ("DELETE", group(), None),
        "team_join": lambda: ("POST", group() + "/join", None),
        "team_leave": lambda: ("POST", group() + "/leave", None),
        "team_list": lambda: ("GET", f"/api/v1/teams?tab={s['tab']}", None),
        "project_create": lambda: ("POST", "/api/v1/projects",
                                   {"team_id": team_id(s["team"]), **fields(*project_fields)}),
        "project_edit": lambda: ("PATCH", group(), fields(*project_fields)),
        "project_delete": lambda: ("DELETE", group(), None),
        "project_join": lambda: ("POST", group() + "/join", None),
        "project_leave": lambda: ("POST", group() + "/leave", None),
        "project_list": lambda: ("GET", f"/api/v1/projects?tab={s['tab']}", None),
        "sdk_setup": lambda: ("GET", group() + "/sdk-setup", None),
        "accept_new_user": lambda: (
            "POST", (f"/api/v1/teams/{team_id(s['team'])}" if s["group"] == "team"
                     else f"/api/v1/projects/{project_id(s['project'])}") + "/members/accept-new-user",
            {"token": token_for(s["token_for"])}),
        "reset_password": lambda: ("POST", f"/api/v1/auth/reset-password/{token_for(s['token_for'])}",
                                   {"password": s["password"]}),
    }
    service = lambda: one(  # noqa
        f"SELECT id FROM alerts_messagingserviceconfig WHERE project_id = {project_id(s['project'])} "
        f"AND display_name = '{s['service']}' ORDER BY id LIMIT 1") or "999999"
    config = lambda: {k: v.replace("@CAPTURE@", capture) for k, v in s.get("config", {}).items()}  # noqa
    services = lambda: group() + "/messaging-services"  # noqa
    calls.update({
        "service_add": lambda: ("POST", services(), {"display_name": s["display_name"], "kind": s["kind"],
                                                     "config": config()}),
        "service_edit": lambda: ("PATCH", f"{services()}/{service()}", {"display_name": s["display_name"],
                                                                         "config": config()}),
        "service_remove": lambda: ("DELETE", f"{services()}/{service()}", None),
        "service_test": lambda: ("POST", f"{services()}/{service()}/test", None),
        "services": lambda: ("GET", services(), None),
    })
    calls.update({
        "signup": lambda: ("POST", "/api/v1/auth/signup", fields("email", "password")),
        "confirm_email": lambda: ("POST", f"/api/v1/auth/confirm-email/{token_for(s['token_for'])}", None),
        "resend_confirmation": lambda: ("POST", "/api/v1/auth/resend-confirmation", fields("email")),
        "request_reset_password": lambda: ("POST", "/api/v1/auth/request-reset-password", fields("email")),
        "preferences": lambda: ("PATCH", "/api/v1/me", fields("send_email_alerts", "theme_preference", "language")),
        "change_password": lambda: ("POST", "/api/v1/me/password", fields("old_password", "new_password")),
    })
    token = lambda: one(f"SELECT id FROM bsmain_authtoken WHERE description = '{s['token']}' "  # noqa
                        "ORDER BY id DESC LIMIT 1") or "999999"
    calls.update({
        "home": lambda: ("GET", "/api/v1/home", None),
        "users": lambda: ("GET", "/api/v1/users", None),
        "user_edit": lambda: ("PATCH", f"/api/v1/users/{user_id(s['user'])}", fields("email")),
        "user_activate": lambda: ("POST", f"/api/v1/users/{user_id(s['user'])}/activate", None),
        "user_deactivate": lambda: ("POST", f"/api/v1/users/{user_id(s['user'])}/deactivate", None),
        "user_delete": lambda: ("DELETE", f"/api/v1/users/{user_id(s['user'])}", None),
        "token_create": lambda: ("POST", "/api/v1/auth-tokens", {
            "description": s["description"], "is_user_bound": s.get("is_user_bound", False),
            "project_id": int(project_id(s["project"])) if s.get("project") else None,
            "capabilities": s["capabilities"]}),
        "token_list": lambda: ("GET", "/api/v1/auth-tokens", None),
        "token_revoke": lambda: ("DELETE", f"/api/v1/auth-tokens/{token()}", None),
        "counts": lambda: ("GET", "/api/v1/counts", None),
        "settings": lambda: ("GET", "/api/v1/settings", None),
        "system_warnings": lambda: ("GET", "/api/v1/system-warnings", None),
        "silence_email": lambda: ("POST", "/api/v1/system-warnings/email/silence", None),
        "silence_remote": lambda: ("POST", f"/api/v1/system-warnings/remote/{s['key']}/silence", None),
    })
    for k in ("team", "project"):
        calls.update({
            f"{k}_invite": lambda: ("POST", group() + "/members", fields("email", "role")),
            f"{k}_members": lambda: ("GET", group() + "/members", None),
            f"{k}_remove": lambda: ("DELETE", f"{group()}/members/{user_id(s['user'])}", None),
            f"{k}_reinvite": lambda: ("POST", f"{group()}/members/{user_id(s['user'])}/reinvite", None),
            f"{k}_invite_link": lambda: ("GET", f"{group()}/members/{user_id(s['user'])}/invite-link", None),
            f"{k}_member_settings": lambda: ("PATCH", f"{group()}/members/{user_id(s['user'])}",
                                             fields("role", "send_email_alerts")),
            f"{k}_accept": lambda: ("POST", group() + "/accept", None),
            f"{k}_decline": lambda: ("POST", group() + "/decline", None),
        })
    method, path, body = calls[op]()
    status, response, cookie = call(method, path, body, me)
    if op in ("reset_password", "confirm_email", "signup") and status == 200:  # logged in
        sessions[s["as"]] = (cookie, response["csrf_token"])

    result = {"n": s["n"], "outcome": None, "errors": None, "link": None, "read": None, "needs_password": None}
    result["outcome"] = {400: "invalid", 403: "forbidden", 404: "not_found", 409: "refused"}.get(
        status, "ok" if 200 <= status < 300 else f"error: {status}")
    if status == 400:
        result["errors"] = response.get("errors")
    if status == 200 and isinstance(response, dict):
        result["link"] = response.get("invite_link")
        if op == "accept_new_user":
            result["needs_password"] = response["needs_password"]
    if status == 200:
        result["read"] = read(op, response)
    wait_for_jobs()
    result["snapshot"] = snapshot()
    return result


def read(op, r):
    emails = {int(i): e for i, e in (row.split("|") for row in psql("SELECT id || '|' || email FROM auth_user"))}
    my = lambda x: (  # noqa
        [x["my_role"], x["my_accepted"], x["my_send_email_alerts"]] if x["my_role"] is not None else None)
    if op == "team_list":
        return sorted([t["name"], t["project_count"], t["member_count"], my(t)] for t in r["items"])
    if op == "project_list":
        return sorted([p["name"], p["team_name"], p["member_count"], p["open_issue_count"], my(p)] for p in r["items"])
    if op in ("team_members", "project_members"):
        return sorted([m["email"], m["role"], m["accepted"], m["send_email_alerts"], m["is_active"]]
                      for m in r["members"])
    if op == "sdk_setup":
        return r["dsn"]
    if op == "home":
        return f"issues/{r['project_id']}" if r["go"] == "issues" else r["go"]
    if op == "users":
        return [[u["username"], u["email"], u["is_active"], u["is_superuser"]] for u in r]
    if op == "token_list":
        return sorted([t["description"], t["is_user_bound"], emails.get(t["user"]["id"]) if t["user"] else None,
                       t["project"]["name"] if t["project"] else None, sorted(t["capabilities"]),
                       t["has_all_capabilities"]] for t in r)
    if op == "counts":
        return {app: models for app, models in r.items() if app != "jobs"}
    if op == "system_warnings":
        return [[[[p["text"], p["url"]] for p in w["parts"]] if w["parts"] is not None else w["message"],
                 w["silenceable"]] for w in r]
    if op == "services":
        return [[sc["display_name"], sc["kind"], sc["config"]] for sc in r["services"]]
    return None


for line in sys.stdin:
    step = json.loads(line)
    if mode in step.get("modes", [mode]):
        print("STEP " + json.dumps(run(step)), flush=True)
