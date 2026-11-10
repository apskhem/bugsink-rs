"""Makes a capture.jsonl (see capture.py) comparable across runs: issue ids are random, and the order of independent
sends (several recipients, several services) is not significant. Usage: normalize_capture.py < capture.jsonl"""
import json
import re
import sys

UUID = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")

entries = [json.loads(UUID.sub("<uuid>", line)) for line in sys.stdin if line.strip()]
for entry in sorted(json.dumps(e, sort_keys=True, ensure_ascii=False) for e in entries):
    print(entry)
