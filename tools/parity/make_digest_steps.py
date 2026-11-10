"""Generates tools/parity/digest-steps.jsonl: a deterministic sequence of events to digest, interleaved with SQL
"user actions" (resolve, mute for/until, grouping-mechanism upgrade, retention changes) applied identically to upstream and to us.
Also sets up alert recipients and messaging services."""
import json
import random
import uuid
from datetime import datetime, timedelta, timezone

random.seed(7)
T0 = datetime(2026, 11, 1, tzinfo=timezone.utc)
KEYS = {1: "11111111111111111111111111111111", 2: "22222222222222222222222222222222"}


def at(i):
    return (T0 + timedelta(minutes=7 * i)).isoformat()


def project_sql(pid, mechanism, retention):
    return (
        "INSERT INTO projects_project (id, name, slug, sentry_key, is_deleted, has_releases, issue_count, "
        "digested_event_count, stored_event_count, alert_on_new_issue, alert_on_regression, alert_on_unmute, visibility, "
        "quota_exceeded_reason, next_quota_check, retention_max_event_count, grouping_mechanism) VALUES "
        f"({pid}, 'p{pid}', 'p{pid}', '{KEYS[pid]}', false, false, 0, 0, 0, false, false, false, 99, 'null', 0, {retention}, "
        f"'{mechanism}')"
    )


# irrelevance_for_retention is random, which would make eviction pick different events on each side. This trigger
# replaces it with a function of the event_id, so both sides evict the same events. Also on UPDATE: upstream re-saves
# all fields of never_evict events. The wide range (0-255) avoids ties: among equally irrelevant events with the same
# digest_order, which one goes first depends on the physical row order, which differs between the two sides.
IRRELEVANCE_TRIGGER = [
    "CREATE FUNCTION parity_irrelevance() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN "
    "NEW.irrelevance_for_retention := get_byte(decode(md5(NEW.event_id::text), 'hex'), 0); RETURN NEW; END $$",
    "CREATE TRIGGER parity_irrelevance BEFORE INSERT OR UPDATE ON events_event "
    "FOR EACH ROW EXECUTE FUNCTION parity_irrelevance()",
]

steps = [{"sql": project_sql(1, "bugsink-v1", 10000)}, {"sql": project_sql(2, "bugsink-v2", 15)}]
steps += [{"sql": sql} for sql in IRRELEVANCE_TRIGGER]

# Alerts. Who gets an email: (email, is_active, user pref, project 2 membership (accepted, pref) or None,
# team membership (accepted, pref) or None, expected to get it) -- the last column is documentation only.
TEAM = "00000000-0000-0000-0000-00000000a001"
USERS = [
    ("u1@example.org", True, True, (True, None), None, True),
    ("u2@example.org", True, False, (True, None), None, False),
    ("u3@example.org", True, False, (True, True), None, True),
    ("u4@example.org", True, True, (True, False), None, False),
    ("u5@example.org", True, False, (True, None), (True, True), True),
    ("u6@example.org", True, True, (True, None), (True, False), False),
    ("u7@example.org", True, True, (True, None), (True, None), True),
    ("u8@example.org", False, True, (True, None), None, False),  # inactive
    ("u9@example.org", True, True, (False, None), None, False),  # invitation not accepted
    ("", True, True, (True, None), None, False),  # no email address
    ("u11@bugsink.com", True, True, (True, None), None, False),  # refused: not Bugsink's own installation
    ("u12@example.org", True, False, (True, None), (False, True), False),  # team invitation not accepted
]


def sql_bool(b):
    return "NULL" if b is None else ("true" if b else "false")


steps.append({"sql": "UPDATE projects_project SET alert_on_regression = true, alert_on_unmute = true WHERE id = 1"})
steps.append({"sql": f"INSERT INTO teams_team (id, name, visibility) VALUES ('{TEAM}', 'team', 1)"})
steps.append({"sql": "UPDATE projects_project SET alert_on_new_issue = true, alert_on_regression = true, "
                     f"alert_on_unmute = true, team_id = '{TEAM}', name = 'Shop \"Front\" & <Co>' WHERE id = 2"})
for uid, (address, active, pref, pm, tm, _) in enumerate(USERS, start=1):
    steps.append({"sql": "INSERT INTO auth_user (id, password, is_superuser, username, first_name, last_name, email, "
                         "is_staff, is_active, date_joined, send_email_alerts, theme_preference, language) VALUES "
                         f"({uid}, '!', false, 'user{uid}', '', '', '{address}', false, {sql_bool(active)}, "
                         f"'2026-10-01T00:00:00Z', {sql_bool(pref)}, 'system', 'auto')"})
    steps.append({"sql": "INSERT INTO projects_projectmembership (id, send_email_alerts, role, accepted, project_id, "
                         f"user_id) VALUES ({uid}, {sql_bool(pm[1])}, 0, {sql_bool(pm[0])}, 2, {uid})"})
    if tm is not None:
        steps.append({"sql": "INSERT INTO teams_teammembership (id, send_email_alerts, role, accepted, team_id, user_id) "
                             f"VALUES ({uid}, {sql_bool(tm[1])}, 0, {sql_bool(tm[0])}, '{TEAM}', {uid})"})
steps.append({"sql": "INSERT INTO projects_projectmembership (id, send_email_alerts, role, accepted, project_id, "
                     "user_id) VALUES (100, NULL, 0, true, 1, 1)"})

# Messaging services; the harness replaces @CAPTURE_HTTP@ with the capture server's address as each side sees it.
SERVICES = [
    (1, "slack", '{"webhook_url": "http://@CAPTURE_HTTP@/slack/p1"}'),
    (2, "slack", '{"webhook_url": "http://@CAPTURE_HTTP@/slack/p2"}'),
    (2, "discord", '{"webhook_url": "http://@CAPTURE_HTTP@/discord"}'),
    (2, "mattermost", '{"webhook_url": "http://@CAPTURE_HTTP@/mattermost", "channel": "town-square"}'),
    (2, "msteams", '{"webhook_url": "http://@CAPTURE_HTTP@/msteams"}'),
    (2, "google_chat", '{"webhook_url": "http://@CAPTURE_HTTP@/google_chat"}'),
    (2, "custom", '{"webhook_url": "http://@CAPTURE_HTTP@/custom"}'),
    (2, "slack", '{"webhook_url": "http://@CAPTURE_HTTP@/fail/slack"}'),  # the capture server answers 500
]
for sid, (pid, kind, config) in enumerate(SERVICES, start=1):
    steps.append({"sql": "INSERT INTO alerts_messagingserviceconfig (id, display_name, kind, config, project_id) "
                         f"VALUES ({sid}, '{kind} {sid}', '{kind}', '{config}', {pid})"})

types = ["ValueError", "KeyError", "TypeError", "ZeroDivisionError", "RuntimeError", "IntegrityError"]
templates = ["user {} not found", "invalid literal for int(): '{}'", "timeout after {}ms", "row {} missing",
             "connect to 10.0.0.{} refused", "static message"]
transactions = ["/api/users", "/api/orders", "checkout", None]
release_phases = ["", "1.0.0", "1.1.0", "abc123", "pkg@2.0.0", "2.0.0-rc.1", "release/with/slashes"]

def by_fingerprint(key):
    return f"(SELECT issue_id FROM issues_grouping WHERE project_id = 1 AND grouping_key = '{key}')"


def mute_until(value, period, nr_of_periods, volume):
    # as upstream's mute-until action: unmute once the volume is reached; the first check happens on the next event
    conditions = json.dumps([{"period": period, "nr_of_periods": nr_of_periods, "volume": volume}])
    return (f"UPDATE issues_issue SET is_muted = true, unmute_on_volume_based_conditions = '{conditions}', "
            f"next_unmute_check = 0 WHERE project_id = 1 AND calculated_value = '{value}'")


actions = {
    30: mute_until("disk 1 full", "hour", 24, 5),
    50: mute_until("disk 2 full", "hour", 1, 2),
    70: mute_until("disk 3 full", "day", 1, 3),
    40: "UPDATE issues_issue SET is_resolved = true, is_resolved_unconditionally = true "
        "WHERE project_id = 1 AND digest_order = 1",
    60: "UPDATE issues_issue SET is_resolved = true, is_resolved_by_next_release = true "
        "WHERE project_id = 1 AND digest_order = 2",
    80: "UPDATE issues_issue SET is_resolved = true, fixed_at = fixed_at || E'1.1.0\\n' "
        f"WHERE id = {by_fingerprint('resolve-me')}",  # regresses on events in later releases
    100: f"UPDATE issues_issue SET is_muted = true, unmute_after = '{at(130)}' WHERE id = {by_fingerprint('mute-me')}",
    185: "UPDATE issues_issue SET is_resolved = true, fixed_at = fixed_at || E'releasewithslashes\\n' "
         f"WHERE id = {by_fingerprint('slash-me')}",  # event.release != sanitized version: upstream drops the event
    120: "UPDATE projects_project SET grouping_mechanism = 'bugsink-v2', previous_grouping_mechanism = 'bugsink-v1', "
         f"grouping_mechanism_upgraded_at = '{at(120)}' WHERE id = 1",
    150: "UPDATE issues_issue SET is_resolved = true WHERE project_id = 2 AND digest_order = 1",
    # eviction: project 2 evicts 1 at a time from the start (max 15); project 1 drops to 50 at once (one big eviction,
    # then 2 at a time); zero retention deletes all of project 2's events; then never_evict events have to go too.
    140: "UPDATE projects_project SET retention_max_event_count = 50 WHERE id = 1",
    170: "UPDATE projects_project SET retention_max_event_count = 0 WHERE id = 2",
    190: "UPDATE projects_project SET retention_max_event_count = 3 WHERE id = 2",
}

event_ids = []
for i in range(200):
    if i in actions:
        steps.append({"sql": actions[i]})

    project_id = 1 if random.random() < 0.8 else 2
    release = release_phases[min(i // 30, len(release_phases) - 1)]
    event = {"platform": random.choice(["python", "javascript", "native", None]), "level": "error",
             "sdk": {"name": "sentry.python", "version": "2.0.0"}}
    if release:
        event["release"] = release
    if (t := random.choice(transactions)) is not None:
        event["transaction"] = t

    kind = random.random()
    if kind < 0.6:
        ty = random.choice(types)
        value = random.choice(templates).format(random.randint(1, 4))
        frames = [{"function": "main", "filename": "app.py", "module": "app", "in_app": True},
                  {"function": random.choice(["handler", "std::vector<int>::at(int) const"]), "filename": "lib.py"}]
        event["exception"] = {"values": [{"type": ty, "value": value, "stacktrace": {"frames": frames},
                                          "mechanism": {"handled": random.random() < 0.5}}]}
    elif kind < 0.8:
        event["logentry"] = {"message": "disk %s full", "formatted": f"disk {random.randint(1, 3)} full"}
    elif kind < 0.92:
        event["message"] = "fingerprinted"
        event["fingerprint"] = random.choice(
            [["custom-group"], ["{{ default }}", "tenant-a"], ["resolve-me"], ["mute-me"], ["slash-me"]])
    else:
        event["exception"] = {"values": [{"type": "SIGSEGV", "mechanism": {"synthetic": True},
                                          "stacktrace": {"frames": [{"function": "crash_here(int)"}]}}]}

    if random.random() < 0.5:
        event["tags"] = random.choice([{"region": "eu", "custom": True}, [["region", "us"], ["shard", 3]]])
    if random.random() < 0.5:
        event["user"] = random.choice([{"id": 42, "ip_address": "{{auto}}"}, {"email": "a@example.com"}])
    if random.random() < 0.3:
        event["contexts"] = {"browser": {"name": "Firefox", "version": "120"}, "trace": {"trace_id": "t1", "span_id": "s1"}}
    if random.random() < 0.3:
        event["request"] = {"url": "https://example.com/x?y=1#z"}
    if random.random() < 0.3:
        event["timestamp"] = random.choice([1793491200.25, "2026-11-01T01:02:03Z", "2026-11-01 01:02:03"])
    event["environment"] = random.choice(["production", "staging", ""])

    event_id = uuid.UUID(int=random.getrandbits(128)).hex
    if i in (33, 77):
        event_id = event_ids[-1]  # duplicate: must be ignored
    if i == 90:
        event["exception"] = "not a list or dict"  # upstream drops this event
    event_ids.append(event_id)

    steps.append({"n": i, "project_id": project_id, "event_id": event_id, "ingested_at": at(i),
                  "digested_at": (T0 + timedelta(minutes=7 * i, seconds=2)).isoformat(),
                  "remote_addr": "203.0.113.5", "event": event})

with open("tools/parity/digest-steps.jsonl", "w") as f:
    for s in steps:
        f.write(json.dumps(s) + "\n")
print(len(steps), "steps")
