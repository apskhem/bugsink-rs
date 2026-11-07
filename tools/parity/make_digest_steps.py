"""Generates tools/parity/digest-steps.jsonl: a deterministic sequence of events to digest, interleaved with SQL
"user actions" (resolve, mute, grouping-mechanism upgrade) applied identically to upstream and to us."""
import json
import random
import uuid
from datetime import datetime, timedelta, timezone

random.seed(7)
T0 = datetime(2026, 11, 1, tzinfo=timezone.utc)
KEYS = {1: "11111111111111111111111111111111", 2: "22222222222222222222222222222222"}


def at(i):
    return (T0 + timedelta(minutes=7 * i)).isoformat()


def project_sql(pid, mechanism):
    return (
        "INSERT INTO projects_project (id, name, slug, sentry_key, is_deleted, has_releases, issue_count, "
        "digested_event_count, stored_event_count, alert_on_new_issue, alert_on_regression, alert_on_unmute, visibility, "
        "quota_exceeded_reason, next_quota_check, retention_max_event_count, grouping_mechanism) VALUES "
        f"({pid}, 'p{pid}', 'p{pid}', '{KEYS[pid]}', false, false, 0, 0, 0, false, false, false, 99, 'null', 0, 10000, "
        f"'{mechanism}')"
    )


steps = [{"sql": project_sql(1, "bugsink-v1")}, {"sql": project_sql(2, "bugsink-v2")}]

types = ["ValueError", "KeyError", "TypeError", "ZeroDivisionError", "RuntimeError", "IntegrityError"]
templates = ["user {} not found", "invalid literal for int(): '{}'", "timeout after {}ms", "row {} missing",
             "connect to 10.0.0.{} refused", "static message"]
transactions = ["/api/users", "/api/orders", "checkout", None]
release_phases = ["", "1.0.0", "1.1.0", "abc123", "pkg@2.0.0", "2.0.0-rc.1", "release/with/slashes"]

def by_fingerprint(key):
    return f"(SELECT issue_id FROM issues_grouping WHERE project_id = 1 AND grouping_key = '{key}')"


actions = {
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
