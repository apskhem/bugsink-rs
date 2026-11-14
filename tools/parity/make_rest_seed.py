"""Seeds the database rest_api.sh compares the REST API on, after the digest steps (and reads.sh's extras): replay steps
(JSONL, to stdout) for issues and events enough to paginate, and the users, teams, projects and auth tokens the
requests use (written to the file given as the first argument, as SQL).

Tokens are named in rest-steps.jsonl; their values are TOKENS[name] (40 hex characters)."""
import json
import sys
from datetime import datetime, timedelta, timezone

T0 = datetime(2026, 11, 2, tzinfo=timezone.utc)
TEAM, HIDDEN_TEAM, OPEN_TEAM = ("00000000-0000-0000-0000-00000000a001", "00000000-0000-0000-0000-00000000a002",
                                "00000000-0000-0000-0000-00000000a003")
ALL = ["issues_read", "events_read", "issues_comment", "issues_triage", "issues_delete", "releases_read",
       "releases_create", "debug_files_upload", "projects_read", "projects_manage", "teams_read", "teams_manage"]
PROJECT_BOUNDABLE = [c for c in ALL if c not in ("projects_manage", "teams_read", "teams_manage")]

# name: (user id or None, project id or None, capabilities, expires_at SQL, is_user_bound override)
TOKEN_SPECS = {
    "full": (None, None, ALL, "NULL"),
    "releases_only": (None, None, ["releases_read"], "NULL"),
    "p1": (None, 1, PROJECT_BOUNDABLE, "NULL"),
    "p2_read": (None, 2, ["issues_read", "events_read", "projects_read", "releases_read"], "NULL"),
    "deleted_project": (None, 3, PROJECT_BOUNDABLE, "NULL"),
    "expired": (None, None, ALL, "now() - interval '1 day'"),
    "expiring": (None, None, ALL, "now() + interval '30 days'"),
    "admin": (60, None, ALL, "NULL"),
    "u1": (1, None, ALL, "NULL"),
    "u5": (5, None, ALL, "NULL"),
    "u9": (9, None, ALL, "NULL"),
    "teamadmin": (61, None, ALL, "NULL"),
    "outsider": (62, None, ALL, "NULL"),
    "inactive": (8, None, ALL, "NULL"),
    "u1_p2": (1, 2, PROJECT_BOUNDABLE, "NULL"),
}
TOKENS = {name: f"{i + 1:040x}".replace("0", "c", 8) for i, name in enumerate(TOKEN_SPECS)}


def replay_steps():
    n = 3000
    # 300 issues of one event each; digested in threes at the same moment (ties for the cursor's offset)
    for i in range(300):
        at = T0 + timedelta(seconds=10 * (i // 3))
        event = {"message": f"many {i}", "fingerprint": [f"many-{i}"], "level": "warning", "platform": "python",
                 "tags": {"batch": f"b{i // 100}", "parity": "yes" if i % 2 else "no"}}
        yield {"n": n, "project_id": 1, "event_id": f"{0xa11000 + i:032x}", "ingested_at": at.isoformat(),
               "digested_at": at.isoformat(), "remote_addr": "203.0.113.9", "event": event}
        n += 1
    # one issue with 600 events
    for i in range(600):
        at = T0 + timedelta(hours=2, seconds=i)
        event = {"message": "pages", "fingerprint": ["pages"], "level": "error", "platform": "javascript",
                 "tags": {"page": f"x{i % 4}"}, "release": "1.0.0" if i < 300 else "2.0.0",
                 "exception": {"values": [{"type": "Error", "value": f"page {i}", "stacktrace": {"frames": [
                     {"filename": "app.js", "function": "go", "lineno": 10 + i % 3, "in_app": True,
                      "pre_context": ["a", "b"], "context_line": "c", "post_context": ["d"],
                      "vars": {"i": i, "s": "x", "o": {"k": [1, None, True]}}}]}}]}}
        yield {"n": n, "project_id": 1, "event_id": f"{0xb22000 + i:032x}", "ingested_at": at.isoformat(),
               "digested_at": at.isoformat(), "remote_addr": "203.0.113.9", "event": event}
        n += 1


def seed_sql():
    yield (f"INSERT INTO teams_team (id, name, visibility) VALUES ('{HIDDEN_TEAM}', 'hidden team', 99), "
           f"('{OPEN_TEAM}', 'open team', 10)")
    for pid, name, team, visibility, deleted in [(3, "deleted", TEAM, 99, True),
                                                 (4, "discoverable", OPEN_TEAM, 10, False),
                                                 (5, "hidden", HIDDEN_TEAM, 99, False)]:
        yield ("INSERT INTO projects_project (id, name, slug, sentry_key, is_deleted, has_releases, issue_count, "
               "digested_event_count, stored_event_count, alert_on_new_issue, alert_on_regression, alert_on_unmute, "
               "visibility, quota_exceeded_reason, next_quota_check, retention_max_event_count, grouping_mechanism, "
               f"team_id) VALUES ({pid}, '{name}', 'p{pid}', '{pid:032x}', {str(deleted).lower()}, false, 0, 0, 0, "
               f"true, true, false, {visibility}, 'null', 0, 10000, 'bugsink-v2', '{team}')")
    yield "SELECT setval('projects_project_id_seq', 10)"
    for uid, name, superuser in [(60, "admin", True), (61, "teamadmin", False), (62, "outsider", False)]:
        yield ("INSERT INTO auth_user (id, password, is_superuser, username, first_name, last_name, email, is_staff, "
               "is_active, date_joined, send_email_alerts, theme_preference, language) VALUES "
               f"({uid}, '!', {str(superuser).lower()}, '{name}@example.org', '', '', '{name}@example.org', false, "
               "true, '2026-10-01T00:00:00Z', true, 'system', 'auto')")
    yield ("INSERT INTO teams_teammembership (id, send_email_alerts, role, accepted, team_id, user_id) VALUES "
           f"(61, NULL, 1, true, '{TEAM}', 61), (62, NULL, 0, true, '{HIDDEN_TEAM}', 9)")
    yield ("INSERT INTO projects_projectmembership (id, send_email_alerts, role, accepted, project_id, user_id) VALUES "
           "(161, NULL, 1, true, 4, 61), (162, NULL, 0, true, 5, 5)")
    yield "SELECT setval('teams_teammembership_id_seq', 100)"
    yield "SELECT setval('projects_projectmembership_id_seq', 200)"
    for name, (user, project, caps, expires) in TOKEN_SPECS.items():
        columns = ", ".join(ALL)
        values = ", ".join("true" if c in caps else "false" for c in ALL)
        yield ("INSERT INTO bsmain_authtoken (token, description, created_at, is_user_bound, user_id, "
               f"is_project_bound, project_id, expires_at, {columns}) VALUES ('{TOKENS[name]}', '{name}', "
               f"'2026-10-01T00:00:00Z', {str(user is not None).lower()}, {user or 'NULL'}, "
               f"{str(project is not None).lower()}, {project or 'NULL'}, {expires}, {values})")
    # a service token with a user: inconsistent, so refused
    yield ("INSERT INTO bsmain_authtoken (token, description, created_at, is_user_bound, user_id, is_project_bound, "
           f"project_id, expires_at, {', '.join(ALL)}) VALUES ('{'e' * 40}', 'inconsistent', '2026-10-01T00:00:00Z', "
           f"false, 60, false, NULL, NULL, {', '.join(['true'] * len(ALL))})")


if __name__ == "__main__":
    with open(sys.argv[1], "w") as f:
        for sql in seed_sql():
            f.write(sql + ";\n")
    for step in replay_steps():
        print(json.dumps(step))
