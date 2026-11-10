# Runs inside the official Bugsink image (`bugsink-manage shell`): prints upstream's User-Agent-enriched contexts for
# JSONL events on stdin.
import json
import logging
import sys

from events.ua_stuff import get_contexts_enriched_with_ua

logging.disable(logging.WARNING)  # failures are expected for some inputs; their output (unchanged contexts) is compared
for line in sys.stdin:
    print(json.dumps(get_contexts_enriched_with_ua(json.loads(line))))
