# Runs inside upstream's `bugsink-manage shell` for reads.sh, on a database with the digest steps replayed. Prints one
# JSON line per input with upstream's answer, for every read the UI API ports:
#   parse / issues / events: search (parse_query, search_issues, search_events)
#   detail: an issue's header (friendly_id, title, fixed_at, events_at, the project's latest release)
#   tags: Issue._get_issue_tags at both cut-offs
#   groupings: issue_grouping's order
#   history: turningpoint_set_all() with the template's user display name and gravatar hash
#   nav: _first_last / _get_event prev and next / _event_count, within searches
#   list_sparklines: get_issue_list_event_sparklines / get_project_list_event_sparklines counts
#   hourly: get_issue_event_sparkline's queries (hourly counts; with a search, the matching events per hour)
#   display: what issue_event_stacktrace / _details / _breadcrumbs hand to their templates, for every event
import json
from datetime import datetime, timedelta, timezone as dt_timezone

from django.db.models import Count, Max
from django.db.models.functions import TruncHour
from django.http import Http404

from events.models import Event, IssueEventCountsPerHour
from events.sparklines import get_issue_list_event_sparklines, get_project_list_event_sparklines
from issues.models import Issue
from issues.views import (
    GROUPING_MECHANISMS, MECHANISM_INDEPENDENT_GROUPING, _event_count, _first_last, _get_event)
from projects.models import Project
from tags.models import TagValue
from tags.search import parse_query, search_events, search_events_optimized, search_issues
from theme.templatetags.user import best_displayname, gravatar_sha

NOW = datetime(2026, 11, 1, 20, 30, tzinfo=dt_timezone.utc)  # inside the steps' day: partly filled windows
SINCE = datetime(2026, 10, 31, tzinfo=dt_timezone.utc)


def quote(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def iso(dt):
    return dt.isoformat() if dt is not None else None


PARSE_ONLY = [
    "", " ", "plain", "a:b", "a:b c", "c a:b", "  a:b  c  ", "a:", ":b", "a::b", "a:b:c", '"a:b":c', 'a:"b c"',
    'a:"b c" d', 'a:"unclosed', '"unclosed a:b', 'a:b"', 'a"b":c', '"":x', 'x:""', '\\"a:b', 'a:\\"b\\"', 'k:"\\\\"',
    'k:"x\\', "k:1 j:2 k:3", "a:b\tc:d\ne", "a:b\u00a0c", "a:b\u2003c:d", "a:b\u200bc", "a:b\x1cc:d", "a:b\x85c",
    "é:ü text", "日本:語 検索", "a:b\\ c", "\\\\:x", '"a" "b"', '""', '"', '\\', "::", "a:b:", "x : y",
]
PLAIN = ["Error", "error", "ERROR", "disk", "disk 1", "row 3 missing", "int()", "'2'", "%", "_", "\\", "10.0.0",
         "Zero", "nothing like this", "crash_here", "fingerprinted", "   Error   "]


def emit(inp, result):
    print(json.dumps({"input": inp, "result": result}))


def guarded(f):
    try:
        return f()
    except ValueError as e:  # a search of only whitespace / skipped terms: _and_join([])
        return f"error: {e}"


class FakeRequest:  # what _event_count reads
    def __init__(self, q):
        self.GET = {"q": q}


for q in PARSE_ONLY + PLAIN:
    parsed = parse_query(q)
    emit({"kind": "parse", "q": q}, {"tags": list(parsed.tags.items()), "plain_text": parsed.plain_text})

grouping_order = {m: i for i, m in enumerate(
    [MECHANISM_INDEPENDENT_GROUPING] + [m.identifier for m in reversed(GROUPING_MECHANISMS)])}

for project in Project.objects.order_by("id"):
    values = list(TagValue.objects.filter(project=project).select_related("key").order_by("key__key", "value"))
    tag_queries = [f"{v.key.key}:{quote(v.value)}" for v in values]
    tag_queries += [f"{quote(v.key.key)}:{v.value}" for v in values if " " not in v.value and '"' not in v.value][:20]
    by_key = {}
    for v in values:
        by_key.setdefault(v.key.key, []).append(v)
    keys = sorted(by_key)
    pairs = [f"{a.key.key}:{quote(a.value)} {b.key.key}:{quote(b.value)}"
             for i, ka in enumerate(keys) for kb in keys[i + 1:] for a in by_key[ka][:2] for b in by_key[kb][:2]]
    queries = (
        PARSE_ONLY + PLAIN + tag_queries + pairs[:60]
        + [f"{t} {p}" for t in tag_queries[:15] for p in ["Error", "disk"]]
        + ["nosuch:tag", "nosuch:tag Error", f"{values[0].key.key}:{quote(values[0].value.upper())}" if values else ""]
    )
    issues = Issue.objects.filter(project=project, is_deleted=False)
    for q in queries:
        emit({"kind": "issues", "project_id": project.id, "q": q},
             guarded(lambda: sorted(i.digest_order for i in search_issues(project, issues, q))))

    busiest = list(issues.order_by("-stored_event_count", "digest_order")[:3])
    nav_queries = [""] + PLAIN[:4] + tag_queries[:12] + pairs[:6] + ["nosuch:tag"]
    for issue in busiest:
        for q in queries[: len(PARSE_ONLY) + len(PLAIN)] + tag_queries[:40] + pairs[:20]:
            emit({"kind": "events", "project_id": project.id, "issue": issue.digest_order, "q": q},
                 guarded(lambda: sorted(e.event_id.hex for e in search_events(project, issue, q))))
        orders = list(Event.objects.filter(issue=issue).order_by("digest_order").values_list("digest_order", flat=True))
        # around the ends and the middle, and outside them (an issue may have no stored events left)
        ends = {orders[0], orders[len(orders) // 2], orders[-1], orders[-1] + 5} if orders else set()
        around = sorted({0, 1} | ends)
        for q in nav_queries:
            def nav():
                qs = search_events_optimized(project, issue, q)
                first, last = _first_last(qs)
                result = {"first": first, "last": last, "count": _event_count(FakeRequest(q), issue, qs), "around": {}}
                for d in around:
                    neighbours = []
                    for direction in ["prev", "next"]:
                        try:
                            neighbours.append(_get_event(qs, issue, None, d, direction, (first, last)).digest_order)
                        except (Event.DoesNotExist, Http404):
                            neighbours.append(None)
                    result["around"][str(d)] = neighbours
                return result
            emit({"kind": "nav", "project_id": project.id, "issue": issue.digest_order, "q": q, "around": around},
                 guarded(nav))
        for q in ["", PLAIN[0]] + tag_queries[:4]:
            hours = [[iso(b), c, d] for b, c, d in IssueEventCountsPerHour.objects.filter(
                issue_id=issue.id, bucket__gte=SINCE, bucket__lt=NOW).order_by("bucket")
                .values_list("bucket", "count", "digest_order")]
            matching = None
            if q:  # get_issue_event_sparkline's overlay query
                matching = [[iso(b), c, d] for b, c, d in search_events(project, issue, q).filter(
                    digested_at__gte=SINCE, digested_at__lt=NOW,
                ).annotate(bucket=TruncHour("digested_at", tzinfo=dt_timezone.utc)).values("bucket").annotate(
                    count=Count("id"), matching_digest_order=Max("digest_order"),
                ).values_list("bucket", "count", "matching_digest_order").order_by("bucket")]
            emit({"kind": "hourly", "project_id": project.id, "issue": issue.digest_order, "q": q,
                  "since": SINCE.isoformat(), "until": NOW.isoformat()}, {"hours": hours, "matching": matching})

    for issue in issues.order_by("digest_order"):
        inp = {"project_id": project.id, "issue": issue.digest_order}
        emit({"kind": "detail", **inp}, {
            "friendly_id": issue.friendly_id(), "title": issue.title(), "fixed_at": issue.get_fixed_at(),
            "events_at": issue.get_events_at(), "latest_release": project.get_latest_release().version})
        for cutoff in [4, 25]:
            result = []
            for entries in issue._get_issue_tags(cutoff, "Other"):
                values = [[e.value.value, e.count, e.pct] for e in entries if not isinstance(e, dict)]
                other = [[e["count"], e["pct"]] for e in entries if isinstance(e, dict)]
                key = next(e.key.key for e in entries if not isinstance(e, dict))
                result.append({"key": key, "values": values, "other": other[0] if other else None})
            emit({"kind": "tags", "cutoff": cutoff, **inp}, result)
        groupers = sorted(issue.grouping_set.all(),
                          key=lambda g: (grouping_order.get(g.grouping_mechanism, 999), g.grouping_key))
        emit({"kind": "groupings", **inp}, [[g.grouping_mechanism, g.grouping_key] for g in groupers])
        emit({"kind": "history", **inp}, [
            [tp.id, tp.kind, iso(tp.timestamp), json.loads(tp.metadata), tp.comment,
             [tp.user_id, best_displayname(tp.user), gravatar_sha(tp.user)] if tp.user_id else None,
             tp.triggering_event.event_id.hex if tp.triggering_event_id else None]
            for tp in issue.turningpoint_set_all().select_related("triggering_event")])

    ids = list(issues.values_list("id", flat=True))
    sparklines = get_issue_list_event_sparklines(ids, NOW, project.id)
    by_order = {i.id: i.digest_order for i in issues}
    counts = {str(by_order[i]): [b["count"] for b in sparklines[i]["event_buckets"]]
              for i in sorted(ids, key=by_order.get)}
    emit({"kind": "list_sparklines", "project_id": project.id, "now": NOW.isoformat()}, counts)

projects = list(Project.objects.order_by("id").values_list("id", flat=True))
emit({"kind": "project_sparklines", "now": NOW.isoformat()},
     {str(p): [b["count"] for b in s["event_buckets"]]
      for p, s in get_project_list_event_sparklines(projects, NOW).items()})


# Event display: upstream's own views, with `render` replaced to capture the template context.
from unittest import mock  # noqa: E402

from django.http import HttpResponse  # noqa: E402
from django.test import RequestFactory  # noqa: E402

import issues.views as views  # noqa: E402
import phonehome.utils  # noqa: E402
from users.models import User  # noqa: E402

captured = {}


def capture(request, template, context):
    captured.clear()
    captured.update(context)
    return HttpResponse()


def incomplete_marks(value, pointer, marks):
    """IncompleteList/IncompleteDict (annotate_with_meta) as {JSON pointer: items trimmed}."""
    if hasattr(value, "incomplete"):
        marks[pointer] = value.incomplete
    if isinstance(value, dict):
        for k, v in value.items():
            incomplete_marks(v, pointer + "/" + str(k).replace("~", "~0").replace("/", "~1"), marks)
    elif isinstance(value, list):
        for i, v in enumerate(value):
            incomplete_marks(v, f"{pointer}/{i}", marks)


def with_marks(entries):
    for entry in entries:
        frames = entry.get("stacktrace", {}).get("frames") if isinstance(entry.get("stacktrace"), dict) else None
        for frame in frames if isinstance(frames, list) else []:
            if isinstance(frame, dict) and "vars" in frame:
                marks = {}
                incomplete_marks(frame["vars"], "", marks)
                if marks:
                    frame["vars_incomplete"] = marks
    return entries


from snappea.settings import get_settings as get_snappea_settings  # noqa: E402

# Viewing a sourcemapped event records file accesses as a task: run it inline (no snappea database here).
get_snappea_settings()["TASK_ALWAYS_EAGER"] = True
admin = User.objects.order_by("id").first()
admin.is_superuser = True  # in memory only: what the views' membership checks look at
rf = RequestFactory()
PARTS = [("stacktrace", views.issue_event_stacktrace), ("details", views.issue_event_details),
         ("breadcrumbs", views.issue_event_breadcrumbs)]
with mock.patch.object(views, "render", capture), \
        mock.patch.object(phonehome.utils, "phone_home_is_due", return_value=False):
    for event in Event.objects.order_by("project_id", "ingested_at", "id"):
        result = {}
        for part, view in PARTS:
            request = rf.get("/")
            request.user = admin
            try:
                view(request, issue_pk=event.issue_id, event_pk=event.id)
            except Exception as e:  # upstream answers 500
                result[part] = f"error: {type(e).__name__}"
                continue
            c = captured
            if part == "stacktrace":
                result[part] = {
                    "entries": with_marks(c["exceptions"]), "thread_stacktraces": with_marks(c["thread_stacktraces"]),
                    "stack_of_plates": c["stack_of_plates"],
                    "multiple_thread_stacktraces": c["multiple_thread_stacktraces"],
                    "request_repr": c["request_repr"]}
            elif part == "details":
                dates = {"timestamp": event.timestamp, "ingested at": event.ingested_at,
                         "digested at": event.digested_at}  # upstream formats these as HTML
                result[part] = {
                    "key_info": [[k, dates[k].isoformat() if k in dates else v] for k, v in c["key_info"]],
                    "logentry_info": c["logentry_info"], "deployment_info": c["deployment_info"],
                    "contexts": c["contexts"]}
            else:
                result[part] = c["breadcrumbs"]
        print(json.dumps({"input": {"kind": "display", "event": str(event.id)}, "result": result}, default=str))
