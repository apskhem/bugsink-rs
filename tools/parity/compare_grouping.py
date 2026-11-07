"""Diffs upstream's and our grouping outputs (JSONL, one line per corpus event). Usage:
compare_grouping.py corpus.jsonl upstream.jsonl ours.jsonl

Cases where upstream raises are reported separately: matching upstream's crash is not a goal, but erroring where
upstream doesn't (or vice versa for grouping keys) is."""
import json
import sys

# split on "\n" only: str.splitlines() would also split on U+2028 etc. inside our (non-ASCII-escaped) output.
corpus, upstream, ours = ([l for l in open(p, encoding="utf-8").read().split("\n") if l] for p in sys.argv[1:4])
upstream = [l for l in upstream if l.startswith("{")]  # drop the Django shell's banner
assert len(corpus) == len(upstream) == len(ours), (len(corpus), len(upstream), len(ours))

mismatches = upstream_only_errors = 0
for i, (event, u_line, o_line) in enumerate(zip(corpus, upstream, ours)):
    u, o = json.loads(u_line), json.loads(o_line)
    for field in ["bugsink-v1", "bugsink-v2", "display", "denormalized"]:
        u_err = isinstance(u[field], dict) and "error" in u[field]
        o_err = isinstance(o[field], dict) and "error" in o[field]
        if u_err and o_err:
            continue
        if u_err:
            upstream_only_errors += 1
            continue
        if u[field] != o[field]:
            mismatches += 1
            print(f"#{i} {field}\n  event:    {event[:200]}\n  upstream: {u[field]}\n  ours:     {o[field]}")

print(f"{len(corpus)} events, {mismatches} mismatches, {upstream_only_errors} fields where only upstream errors")
sys.exit(1 if mismatches else 0)
