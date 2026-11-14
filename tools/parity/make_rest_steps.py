"""Generates tools/parity/rest-steps.jsonl: the requests rest_api.sh sends to upstream's REST API and to ours, in order.

A step: {"n", "method", "path", "token" (a name from make_rest_seed.TOKENS, or null), "headers", "body" (JSON),
"raw" (a body as is)}. In paths and bodies, the driver replaces:
  {created:N}       the "id" in step N's response
  {next:N}, {previous:N}   the path of step N's next/previous link
  {issue:KEY}       the id of the issue with grouping key KEY
  {eventpk:ID}      the (internal) id of the event with event_id ID"""
import json
import uuid
from urllib.parse import quote_plus

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from make_rest_seed import HIDDEN_TEAM, OPEN_TEAM, TEAM  # noqa: E402

API = "/api/canonical/0"
SHOWN = 497  # the display and grouping corpora's events, as rest_api.sh adds them


def shown(i):
    return str(uuid.UUID(int=0xd15000 + i))


steps = []


def step(method, path, token="full", body=None, raw=None, headers=None):
    s = {"n": len(steps) + 1, "method": method, "path": API + path, "token": token}
    if body is not None:
        s["body"] = body
    if raw is not None:
        s["raw"] = raw
    if headers:
        s["headers"] = headers
    steps.append(s)
    return s["n"]


def follow(n, link="next", token="full"):
    steps.append({"n": len(steps) + 1, "method": "GET", "path": f"{{{link}:{n}}}", "token": token})
    return len(steps)


# --- authentication and dispatch
step("GET", "/", token=None)
step("GET", "/")
for header in ["Bearer", "Bearer ", "Bearer x y", "Bearer " + "a" * 21 + " y", "Bearer " + "A" * 40,
               "bearer " + "a" * 40, "Token " + "a" * 40, "Bearer " + "a" * 41, "Bearer " + "f" * 40]:
    step("GET", "/teams/", token=None, headers={"Authorization": header})
for name in ["expired", "inactive", "deleted_project", "expiring"]:
    step("GET", "/teams/", token=name)
step("GET", "/teams/", token=None, headers={"Authorization": "Bearer " + "e" * 40})  # a service token with a user
step("PUT", "/teams/")
step("PUT", "/teams/", token=None)
step("DELETE", f"/teams/{TEAM}/")
step("POST", "/issues/")
step("HEAD", "/issues/P1-1/")
step("OPTIONS", "/issues/")
step("OPTIONS", "/issues/P1-1/resolve/", token="releases_only")
for path in ["/teams/", "/projects/", "/projects/1/", "/issue-comments/", "/releases/", "/issues/P1-1/mute-until/",
             f"/teams/{TEAM}/", "/events/lookup/"]:
    step("OPTIONS", path)
step("OPTIONS", "/teams/", token="u1")
step("GET", "/nope/")
for query in ["", "?format=json", "?format=yaml", "?format=xml"]:
    step("GET", f"/schema/{query}", token=None)
for accept in ["application/json", "application/yaml", "text/html", "*/*"]:
    step("GET", "/schema/", token=None, headers={"Accept": accept})
step("GET", "/schema/swagger-ui/", token=None)

# --- capabilities and bindings
for path in ["/teams/", "/projects/", "/issues/", "/events/?issue=P1-1", "/issue-history/?issue=P1-1",
             "/releases/?project=1", f"/teams/{TEAM}/", "/projects/1/", "/issues/P1-1/"]:
    step("GET", path, token="releases_only")
step("POST", "/issues/P1-1/resolve/", token="p2_read")
step("POST", "/issue-comments/", token="p2_read", body={"issue": "P1-1", "comment": "x"})
for token in ["p1", "p2_read", "u1_p2"]:
    step("GET", "/projects/", token=token)
    step("GET", "/issues/?sort=last_seen", token=token)
    step("GET", "/issues/?project=2", token=token)
    step("GET", "/projects/2/", token=token)
    step("GET", "/issues/P2-1/", token=token)
for token in ["admin", "u1", "u5", "u9", "teamadmin", "outsider"]:
    step("GET", "/teams/", token=token)
    step("GET", "/projects/", token=token)
    step("GET", "/issues/?order=asc", token=token)
    for pid in [1, 2, 3, 4, 5]:
        step("GET", f"/projects/{pid}/", token=token)
    step("GET", "/issues/?project=2", token=token)
    step("GET", "/issues/P2-1/", token=token)
    step("GET", "/events/?issue=P2-1", token=token)
    step("GET", f"/teams/{HIDDEN_TEAM}/", token=token)
    step("GET", f"/teams/{OPEN_TEAM}/", token=token)

# --- issue lists: filters, search, sorting, cursors
p1 = step("GET", "/issues/?project=1")
p1_next = follow(p1)
follow(p1_next, "previous")
follow(p1_next)
many = step("GET", "/issues/?project=1&sort=last_seen&order=desc&q=many")
many_next = follow(many)
follow(many_next, "previous")
counts = step("GET", "/issues/?sort=digested_event_count&order=asc")
counts_next = follow(counts)
follow(counts_next, "previous")
step("GET", "/issues/?sort=digested_event_count")
step("GET", "/issues/?sort=last_seen&order=asc&state=open")
for state in ["open", "unresolved", "resolved", "muted", "x"]:
    step("GET", f"/issues/?project=1&state={state}")
for q in ["batch:b1", "batch:b1 parity:yes", "many 1", "MANY 12", "nonexistent:tag", 'batch:"b2"', "%", "_",
          "parity:no batch:b0 many", "  many 3  "]:
    step("GET", "/issues/?project=1&q=" + quote_plus(q))
    step("GET", "/issues/?q=" + quote_plus(q))
for params in ["project=abc", "project=99", "project=3", "project=", "project=+1", "project=1_0", "sort=digest_order",
               "sort=x", "project=1&order=x", "project=1&sort=last_seen&order=desc&cursor=garbage",
               "project=1&cursor=", "project=1&cursor=bz0xMA", "project=1&cursor=bz0yNTA%3D",
               "project=1&cursor=cj0x", "project=1&sort=digested_event_count&cursor=bz0yJnI9MSZwPTE%3D",
               "project=1&q=many&sort=last_seen&cursor=cD0yMDI2LTExLTAyKzAwJTNBMDUlM0EwMCUyQjAwJTNBMDA%3D",
               "project=1&project=2", "project=1&x=%ZZ&y=a+b&z", "project=1&state=x&sort=x"]:
    step("GET", f"/issues/?{params}")

# --- an issue
for pk in ["P1-1", "p1-1", "{issue:many-7}", "nope", "P1-x", "P9-1", "P1-0", "P1-99999", "P1-1_0", "P1-" + "9" * 30,
           "%7B{issue:many-7}%7D"]:
    step("GET", f"/issues/{pk}/")
for pk in ["P1-1", "{issue:many-7}", "{issue:pages}"]:
    step("GET", f"/issues/{pk}/tags/")
    step("GET", f"/issues/{pk}/grouping/")
step("GET", "/issues/{issue:many-7}/tags/?key=batch")
step("GET", "/issues/{issue:pages}/tags/?key=page")
step("GET", "/issues/{issue:pages}/tags/?key=nope")
for query in ["start=2026-11-02T00:00:00Z&end=2026-11-02T03:00", "start=2026-11-02T00:30:00Z", "start=garbage",
              "start=2026-11-02T02:00:00%2B01:00&end=2026-11-02T04:00Z",
              "start=2026-11-02T03:00Z&end=2026-11-02T03:00Z",
              "start=2026-08-01T00:00Z&end=2026-11-02T00:00Z", "end=2030-01-01T00:00Z", "start=2026-11-01T22:00Z&end=",
              "start=2026-11-02&end=2026-11-02T05", ""]:
    step("GET", f"/issues/{{issue:pages}}/event-counts/?{query}")
    step("GET", f"/projects/1/event-counts/?{query}")

# --- triage
step("POST", "/issues/P1-6/resolve/")
step("POST", "/issues/P1-6/resolve/")
step("POST", "/issues/P1-6/unmute/")
step("POST", "/issues/P1-6/mute/")
step("POST", "/issues/P1-6/reopen/")
step("POST", "/issues/P1-6/reopen/")
step("POST", "/issues/P1-7/resolve-next/", raw="ignored", headers={"Content-Type": "text/plain"})
step("POST", "/issues/P1-8/resolve-latest/")
step("POST", "/issues/P2-2/resolve-latest/")
step("POST", "/issues/{issue:many-1}/resolve-latest/")
step("POST", "/issues/P1-9/mute/")
step("POST", "/issues/P1-9/mute/")
step("POST", "/issues/P1-9/resolve/")
step("POST", "/issues/P1-12/unmute/")
for body in [None, {}, {"period_name": "fortnight", "nr_of_periods": 0}, {"period_name": "day"},
             {"period_name": "day", "nr_of_periods": "x"}, {"period_name": 1, "nr_of_periods": True}, [1], "x", None]:
    if body is None:
        step("POST", "/issues/P1-10/mute-for/")
    else:
        step("POST", "/issues/P1-10/mute-for/", body=body)
step("POST", "/issues/P1-10/mute-for/", raw="{bad")
step("POST", "/issues/P1-10/mute-for/", raw="null")
step("POST", "/issues/P1-10/mute-for/", raw='{"period_name": "day", "nr_of_periods": 3}',
     headers={"Content-Type": "text/plain"})
step("POST", "/issues/P1-10/mute-for/", raw='{"period_name": "day", "nr_of_periods": 3}', headers={"Content-Type": ""})
step("POST", "/issues/P1-10/mute-for/", body={"period_name": "week", "nr_of_periods": "2", "extra": 1})
step("POST", "/issues/P1-10/mute-for/", body={"period_name": "day", "nr_of_periods": 1})
step("POST", "/issues/P1-11/mute-until/", body={"period_name": "hour", "nr_of_periods": 2.0})
step("POST", "/issues/P1-11/mute-until/", body={"period_name": "hour", "nr_of_periods": 2, "gte_threshold": 0})
step("POST", "/issues/P1-11/mute-until/", body={"period_name": "hour", "nr_of_periods": 2, "gte_threshold": " 25 "})
step("POST", "/issues/P1-11/unmute/")
step("POST", "/issues/P1-11/unmute/")
step("POST", "/issues/P1-13/resolve/", token="u9")
step("POST", "/issues/P2-3/resolve/", token="u9")
step("POST", "/issues/P2-3/resolve/", token="u1")
step("POST", "/issues/P2-3/bogus/")
step("DELETE", "/issues/P1-5/", token="p2_read")
step("DELETE", "/issues/P1-5/")
step("DELETE", "/issues/P1-5/")
step("GET", "/issues/P1-5/")
step("GET", "/events/?issue=P1-5")
step("GET", "/issue-history/?issue=P1-6")
step("GET", "/issue-history/?issue=P1-6&order=asc")
step("GET", "/issue-history/?issue=P1-7")

# --- comments and history
for body in [{}, {"issue": "P1-1", "comment": "   "}, {"issue": "P9-1", "comment": "x"},
             {"issue": "zz", "comment": "x"},
             {"issue": "", "comment": ""}, {"issue": True, "comment": 5}, {"issue": None, "comment": None},
             {"issue": "P1-5", "comment": "deleted"}, {"issue": "p1-1", "comment": "  hello  "},
             {"issue": "{issue:many-7}", "comment": "café ☕\nsecond line"}, {"issue": 1, "comment": "x"}]:
    step("POST", "/issue-comments/", body=body)
step("POST", "/issue-comments/", token="u9", body={"issue": "P2-3", "comment": "x"})
step("POST", "/issue-comments/", token="u1", body={"issue": "P2-3", "comment": "member comment"})
step("POST", "/issue-comments/", token="p1", body={"issue": "P2-3", "comment": "x"})
step("GET", "/issue-comments/")
for params in ["issue=P1-1", "issue=P1-1&order=asc", "issue=P1-1&order=x", "issue=", "", "issue=nope", "issue=P1-5",
               "issue=P2-3", "issue={issue:many-7}"]:
    step("GET", f"/issue-history/?{params}")

# --- events
pages = step("GET", "/events/?issue={issue:pages}")
pages_next = follow(pages)
follow(pages_next, "previous")
follow(follow(pages_next))
asc = step("GET", "/events/?issue={issue:pages}&order=asc&q=page:x1")
follow(asc)
for q in ["page:x1 page:x2", "page 1", "nope:x", "Error"]:
    step("GET", "/events/?issue={issue:pages}&q=" + quote_plus(q))
for params in ["issue=", "issue=nope", "issue=P1-1&cursor=zzz", "issue=P1-1&order=x"]:
    step("GET", f"/events/?{params}")
for params in ["project=1", "project=1&event_id=abc", "project=1&event_id=" + "0" * 32, "event_id=" + "0" * 32,
               "project=99&event_id=x", f"project=1&event_id={shown(0)}", f"project=2&event_id={shown(0)}",
               f"project=1&event_id=%7B{shown(1)}%7D", f"project=1&event_id={shown(1).replace('-', '')}"]:
    step("GET", f"/events/lookup/?{params}")
for i in range(SHOWN):
    step("GET", f"/events/lookup/?project=1&event_id={shown(i)}")
for i in range(0, SHOWN, 7):
    step("GET", f"/events/{{eventpk:{shown(i)}}}/stacktrace/")
PAGES_0 = str(uuid.UUID(int=0xb22000))
step("GET", f"/events/{{eventpk:{PAGES_0}}}/")
step("GET", f"/events/{{eventpk:{PAGES_0}}}/stacktrace/", token="releases_only")
step("GET", "/events/abc/stacktrace/")
step("GET", "/events/abc/")
step("GET", "/events/00000000-0000-0000-0000-000000000000/")

# --- releases
step("GET", "/releases/?project=1")
step("GET", "/releases/?project=2&order=asc")
step("GET", "/releases/")
step("GET", "/releases/?project=x")
for body in [{}, {"project": "abc", "version": "", "timestamp": "x"}, {"project": 2, "version": "1.0.0"},
             {"project": True, "version": None}, {"project": 99, "version": "x"}, {"project": 1.0, "version": [1]},
             {"project": "1", "version": " 9.9.9 ", "timestamp": "2026-11-03T10:00:00+02:00"},
             {"project": 1, "version": "9.9.9"}, {"project": 2, "version": "rel/with/slash"},
             {"project": 3, "version": "on-a-deleted-project"}, {"project": 1, "version": "", "timestamp": None}]:
    step("POST", "/releases/", body=body)
step("POST", "/releases/", token="u1", body={"project": 1, "version": "by-member"})
step("POST", "/releases/", token="p1", body={"project": 2, "version": "x"})
step("POST", "/releases/", token="admin", body={"project": 1, "version": "10.0.0", "timestamp": "2026-11-04T00:00Z"})
rel = step("POST", "/releases/", token="p1", body={"project": 1, "version": "10.1.0"})
step("GET", f"/releases/{{created:{rel}}}/")
step("GET", f"/releases/{{created:{rel}}}/", token="p2_read")
step("GET", "/releases/abc/")
step("GET", "/releases/00000000-0000-0000-0000-000000000000/")
step("GET", "/releases/?project=1")
step("GET", "/issues/P1-7/")  # resolved by the next release: now in 9.9.9
step("GET", "/issue-history/?issue=P1-7")

# --- projects
for params in ["?team=" + TEAM, "?team=", "?order=desc", "?expand=team", "?order=x", "?cursor=cD1w"]:
    step("GET", f"/projects/{params}")
for params in ["?expand=team", "?expand=", "?expand=x,team", "?expand=team&expand=x", "?expand=team,team"]:
    step("GET", f"/projects/2/{params}")
for pk in ["abc", "99", "3", "+1"]:
    step("GET", f"/projects/{pk}/")
for body in [{}, {"team": "abc", "name": "x"}, {"team": "00000000-0000-0000-0000-00000000a009", "name": "x"},
             {"team": TEAM, "name": "deleted"}, {"team": None, "name": None}, {"team": 5, "name": True},
             {"team": True, "name": "x" * 256}, {"team": [1], "name": "x"},
             {"team": TEAM, "name": "Brand New!", "retention_max_event_count": -1, "alert_on_new_issue": "maybe"},
             {"team": TEAM, "name": "Brand New!", "retention_max_event_count": 30000},
             {"team": TEAM, "name": "Brand New!", "retention_max_event_count": 19990, "visibility": "x"},
             {"team": TEAM, "name": " Brand New! ", "visibility": "discoverable", "retention_max_event_count": "12",
              "alert_on_unmute": "no", "slug": "ignored", "id": 5}]:
    step("POST", "/projects/", body=body)
step("POST", "/projects/?expand=team", body={"team": TEAM, "name": "x"})
step("POST", "/projects/", body={"team": TEAM, "name": "Brand New!"})
step("POST", "/projects/", body={"team": OPEN_TEAM, "name": "Brand New!"})
step("POST", "/projects/", token="u1", body={"team": TEAM, "name": "by u1"})
step("POST", "/projects/", token="teamadmin", body={"team": TEAM, "name": "by teamadmin"})
step("POST", "/projects/", token="teamadmin", body={"team": OPEN_TEAM, "name": "by teamadmin"})
step("POST", "/projects/", token="p1", body={"team": TEAM, "name": "x"})
for body in [{}, {"name": "deleted"}, {"name": "Shop \"Front\" & <Co>"}, {"name": ""},
             {"team": OPEN_TEAM, "slug": "zz"},
             {"visibility": "joinable", "alert_on_new_issue": False, "retention_max_event_count": 20},
             {"retention_max_event_count": 25000}, {"retention_max_event_count": 22000}]:
    step("PATCH", "/projects/2/", body=body)
step("PATCH", "/projects/1/", body={"name": "p1 renamed"})
step("PATCH", "/projects/3/", body={"name": "still deleted"})
step("PATCH", "/projects/4/", token="u1", body={"name": "x"})
step("PATCH", "/projects/4/", token="teamadmin", body={"name": "discoverable too"})
step("PATCH", "/projects/1/", token="p1", body={"name": "x"})
step("GET", "/projects/?order=desc")
step("GET", "/projects/", token="outsider")

# --- teams
for body in [{}, {"name": "team"}, {"name": "  ", "visibility": "x"}, {"name": 123, "visibility": None},
             {"name": True, "visibility": 1}, {"name": "x" * 256}, {"name": " new team ", "visibility": "hidden"}]:
    step("POST", "/teams/", body=body)
step("POST", "/teams/", token="outsider", body={"name": "by outsider"})
step("POST", "/teams/", token="admin", body={"name": "by admin"})
step("PATCH", f"/teams/{OPEN_TEAM}/", body={"name": "team"})
step("PATCH", f"/teams/{OPEN_TEAM}/", body={"name": "open team", "visibility": "joinable"})
step("PATCH", f"/teams/{OPEN_TEAM}/", body={})
step("PATCH", "/teams/abc/", body={})
step("PATCH", "/teams/00000000-0000-0000-0000-00000000a009/", body={})
step("PATCH", f"/teams/{TEAM}/", token="teamadmin", body={"visibility": "discoverable"})
step("PATCH", f"/teams/{OPEN_TEAM}/", token="teamadmin", body={"visibility": "discoverable"})
step("GET", "/teams/?order=desc")
t = step("GET", "/teams/?cursor=cD1vcGVu&order=desc")
follow(t, "previous")

if __name__ == "__main__":
    with open("tools/parity/rest-steps.jsonl", "w") as f:
        for s in steps:
            f.write(json.dumps(s, ensure_ascii=False) + "\n")
    print(f"{len(steps)} steps")
