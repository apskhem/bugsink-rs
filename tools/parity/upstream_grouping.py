# Runs inside the official Bugsink image (`bugsink-manage shell`): reads JSONL events on stdin, prints upstream's
# grouping outputs as JSONL. Errors are reported, not raised: they are part of the comparison.
import json
import sys

from issues.utils import get_denormalized_fields_for_data, get_key_with_mechanism_for_data, get_type_and_value_for_data


def attempt(f):
    try:
        return f()
    except Exception as e:  # noqa
        return {"error": type(e).__name__}


for line in sys.stdin:
    data = json.loads(line)
    out = {}
    for mechanism in ["bugsink-v1", "bugsink-v2"]:
        out[mechanism] = attempt(lambda: list(get_key_with_mechanism_for_data(json.loads(line), mechanism)))
    out["display"] = attempt(lambda: [str(x) for x in get_type_and_value_for_data(json.loads(line))])
    out["denormalized"] = attempt(lambda: get_denormalized_fields_for_data(json.loads(line)))
    print(json.dumps(out, sort_keys=True))
