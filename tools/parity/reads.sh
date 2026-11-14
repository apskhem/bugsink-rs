#!/usr/bin/env bash
# Compares what the UI API reads with upstream Bugsink (official image): search (tags/search.py) and the issue pages
# (header, tag summaries, groupings, history, event navigation within searches, sparkline counts). The digest steps
# are replayed into a database for realistic data, then upstream builds the inputs from it (plus parser edge cases) and
# both sides answer every one on that same database (upstream_reads.py lists what is compared). Needs docker.
# Usage: tools/parity/reads.sh [image]  (default: bugsink/bugsink:2.6.3)
set -euo pipefail
cd "$(dirname "$0")/../.."
image="${1:-bugsink/bugsink:2.6.3}"
out="$(mktemp -d)"
net=bsrs-parity pg=bsrs-parity-reads-pg port=55497

docker network create "$net" >/dev/null 2>&1 || true
docker rm -f "$pg" >/dev/null 2>&1 || true
docker run -d --rm --name "$pg" --network "$net" -p "$port:5432" -e POSTGRES_PASSWORD=pw postgres:17 >/dev/null
trap 'docker rm -f "$pg" >/dev/null' EXIT
until docker exec "$pg" pg_isready -q -U postgres; do sleep 1; done
docker exec "$pg" createdb -U postgres reads
export DATABASE_URL="postgres://postgres:pw@localhost:$port/reads"

python3 tools/parity/make_digest_steps.py >/dev/null
cargo build -q -p bugsink-server --example replay --example reads_dump
# No messaging services: alerts don't matter here, and there is no capture server to send them to. No retention
# changes or vacuuming either: the more events and tags stay, the more there is to compare.
# Plus a busy issue: more values per tag key than the tags tab shows (25), and "mostly unique" keys (user...).
python3 - > "$out/busy.jsonl" <<'PY'
import json
from datetime import datetime, timedelta, timezone
t0 = datetime(2026, 11, 1, 19, 0, tzinfo=timezone.utc)
for i in range(40):
    at = t0 + timedelta(minutes=i)
    event = {"message": "busy", "fingerprint": ["busy"], "level": "error", "platform": "python",
             "tags": {"build": f"b{i % 30:02d}", "region": ["eu", "us", "ap"][i % 3]}, "user": {"id": f"u{i}"}}
    print(json.dumps({"n": 1000 + i, "project_id": 1, "event_id": f"{i:032x}", "ingested_at": at.isoformat(),
                      "digested_at": (at + timedelta(seconds=1)).isoformat(), "remote_addr": "203.0.113.5",
                      "event": event}))
PY
# Plus events to show: the display corpus and the grouping corpus (what digest drops never gets shown).
python3 tools/parity/make_display_corpus.py >/dev/null
python3 tools/parity/make_grouping_corpus.py >/dev/null
python3 - tools/parity/display-corpus.jsonl tools/parity/grouping-corpus.jsonl > "$out/shown.jsonl" <<'PY'
import json, sys
from datetime import datetime, timedelta, timezone
t0 = datetime(2026, 11, 1, 21, 0, tzinfo=timezone.utc)
lines = [line for path in sys.argv[1:] for line in open(path)]
for i, line in enumerate(lines):
    at = t0 + timedelta(seconds=10 * i)
    print(json.dumps({"n": 2000 + i, "project_id": 1, "event_id": f"{0xd15000 + i:032x}", "ingested_at": at.isoformat(),
                      "digested_at": (at + timedelta(seconds=1)).isoformat(), "remote_addr": "203.0.113.5",
                      "event": json.loads(line)}))
PY
grep -v -e alerts_messagingserviceconfig -e 'SET retention_max_event_count' -e '"vacuum"' \
    tools/parity/digest-steps.jsonl | cat - "$out/busy.jsonl" "$out/shown.jsonl" \
    | ./target/debug/examples/replay >/dev/null

docker run --rm -i --network "$net" -e SECRET_KEY=parity -e "DATABASE_URL=postgres://postgres:pw@$pg:5432/reads" \
    "$image" bugsink-manage shell -c "$(cat tools/parity/upstream_reads.py)" | grep '^{' > "$out/upstream.jsonl"
python3 -c 'import json, sys
for line in sys.stdin: print(json.dumps(json.loads(line)["input"]))' < "$out/upstream.jsonl" \
    | ./target/debug/examples/reads_dump > "$out/ours.jsonl"

python3 - "$out/upstream.jsonl" "$out/ours.jsonl" <<'EOF'
import json, sys


upstream_errors = 0


def tolerant_display(u, o):
    """Where upstream's view fails (a 500), we show what we can: nothing to compare."""
    global upstream_errors
    if u["input"]["kind"] == "display":
        for part, value in u["result"].items():
            if isinstance(value, str) and value.startswith("error: "):
                o["result"][part] = value
                upstream_errors += 1
    return u, o


def tolerant(record):
    """Tag values with equal counts come in the database's order upstream (ours: by id). Sort within runs of equal
    counts; a run cut off by the cut-off can only be compared by size."""
    if record["input"]["kind"] != "tags":
        return record
    for key in record["result"]:
        values, runs = key["values"], {}
        for v in values:
            runs.setdefault(v[1], []).append(v)
        cut = key["other"] is not None and values and values[-1][1]
        key["values"] = [sorted(run) if count != cut else [len(run), count] for count, run in runs.items()]
    return record


upstream = [tolerant(json.loads(line)) for line in open(sys.argv[1])]
ours = [tolerant(json.loads(line)) for line in open(sys.argv[2])]
assert len(upstream) == len(ours), (len(upstream), len(ours))
kinds, diffs = {}, 0
for u, o in (tolerant_display(u, o) for u, o in zip(upstream, ours)):
    kind = u["input"]["kind"]
    kinds[kind] = kinds.get(kind, 0) + 1
    if u != o:
        diffs += 1
        if diffs <= 20:
            print("DIFF", json.dumps(u["input"], ensure_ascii=False))
            print("  upstream:", json.dumps(u["result"], ensure_ascii=False)[:300])
            print("  ours:    ", json.dumps(o["result"], ensure_ascii=False)[:300])
nonempty = sum(1 for u in upstream if isinstance(u["result"], list) and u["result"])
print(f"compared: {kinds}; with matches: {nonempty}; differences: {diffs}; "
      f"event views upstream fails on (not compared): {upstream_errors}")
print("outputs in", sys.argv[1].rsplit("/", 1)[0])
sys.exit(1 if diffs else 0)
EOF
