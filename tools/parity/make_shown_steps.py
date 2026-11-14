"""Replay steps (JSONL, to stdout) digesting every event of the display and grouping corpora, one every 10 seconds
from 2026-11-01 21:00 UTC, so that reads.sh and rest_api.sh have events of every shape to show (what digest drops
never gets shown). Generate the corpora first."""
import json
from datetime import datetime, timedelta, timezone

t0 = datetime(2026, 11, 1, 21, 0, tzinfo=timezone.utc)
lines = [line for path in ["tools/parity/display-corpus.jsonl", "tools/parity/grouping-corpus.jsonl"]
         for line in open(path)]
for i, line in enumerate(lines):
    at = t0 + timedelta(seconds=10 * i)
    print(json.dumps({"n": 2000 + i, "project_id": 1, "event_id": f"{0xd15000 + i:032x}", "ingested_at": at.isoformat(),
                      "digested_at": (at + timedelta(seconds=1)).isoformat(), "remote_addr": "203.0.113.5",
                      "event": json.loads(line)}))
