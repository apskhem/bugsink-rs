# Runs inside the official Bugsink image (`bugsink-manage shell`): replays digest steps (JSONL on stdin) through
# upstream's own digest_event.
import json
import sys
from datetime import datetime

from django.db import connection

from ingest.tasks import digest_event

for line in sys.stdin:
    step = json.loads(line)
    if "sql" in step:
        with connection.cursor() as cursor:
            cursor.execute(step["sql"])
        continue
    metadata = {k: step[k] for k in ["event_id", "project_id", "ingested_at", "remote_addr"]}
    try:
        digest_event(metadata, step["event"], digested_at=datetime.fromisoformat(step["digested_at"]))
    except Exception as e:  # noqa
        print(f"step {step['n']}: {type(e).__name__}: {e}")
