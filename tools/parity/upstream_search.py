# Runs inside upstream's `bugsink-manage shell` for search.sh, on a database with the digest steps replayed: builds
# a query corpus from the tags in the database plus edge cases, and prints one JSON line per query with upstream's
# result (parse_query; search_issues per project; search_events per issue).
import json

from issues.models import Issue
from projects.models import Project
from tags.models import TagValue
from tags.search import parse_query, search_events, search_issues


def quote(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


PARSE_ONLY = [
    "", " ", "plain", "a:b", "a:b c", "c a:b", "  a:b  c  ", "a:", ":b", "a::b", "a:b:c", '"a:b":c', 'a:"b c"',
    'a:"b c" d', 'a:"unclosed', '"unclosed a:b', 'a:b"', 'a"b":c', '"":x', 'x:""', '\\"a:b', 'a:\\"b\\"', 'k:"\\\\"',
    'k:"x\\', "k:1 j:2 k:3", "a:b\tc:d\ne", "a:b\u00a0c", "a:b\u2003c:d", "a:b\u200bc", "a:b\x1cc:d", "a:b\x85c",
    "é:ü text", "日本:語 検索", "a:b\\ c", "\\\\:x", '"a" "b"', '""', '"', '\\', "::", "a:b:", "x : y",
]
PLAIN = ["Error", "error", "ERROR", "disk", "disk 1", "row 3 missing", "int()", "'2'", "%", "_", "\\", "10.0.0",
         "Zero", "nothing like this", "crash_here", "fingerprinted", "   Error   "]

out = []


def emit(inp, result):
    print(json.dumps({"input": inp, "result": result}))


for q in PARSE_ONLY + PLAIN:
    parsed = parse_query(q)
    emit({"kind": "parse", "q": q}, {"tags": list(parsed.tags.items()), "plain_text": parsed.plain_text})

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
        inp = {"kind": "issues", "project_id": project.id, "q": q}
        try:
            emit(inp, sorted(i.digest_order for i in search_issues(project, issues, q)))
        except ValueError as e:  # a query of only whitespace / skipped terms: _and_join([])
            emit(inp, f"error: {e}")
    for issue in issues.order_by("-stored_event_count", "digest_order")[:3]:
        for q in queries[: len(PARSE_ONLY) + len(PLAIN)] + tag_queries[:40] + pairs[:20]:
            inp = {"kind": "events", "project_id": project.id, "issue": issue.digest_order, "q": q}
            try:
                emit(inp, sorted(e.event_id.hex for e in search_events(project, issue, q)))
            except ValueError as e:
                emit(inp, f"error: {e}")
