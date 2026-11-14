# Runs inside the official Bugsink image: prints the User-Agent regexes upstream parses with (ua-parser-builtins, via
# ua-parser's legacy API), as bugsink-server/src/ua_regexes.json. Usage:
#   docker run --rm -i --entrypoint python bugsink/bugsink:2.6.3 - < tools/parity/export_ua_regexes.py \
#       > bugsink-server/src/ua_regexes.json
import importlib.metadata
import json
import re

from ua_parser.user_agent_parser import DEVICE_PARSERS, OS_PARSERS, USER_AGENT_PARSERS


def clean(d):
    return {k: v for k, v in d.items() if v is not None}


print(json.dumps({
    "source": "ua-parser-builtins %s (uap-core regexes), as used by upstream Bugsink; exported from its image"
              % importlib.metadata.version("ua-parser-builtins"),
    "user_agent_parsers": [clean({
        "regex": p.user_agent_re.pattern, "family_replacement": p.family_replacement,
        "v1_replacement": p.v1_replacement, "v2_replacement": p.v2_replacement,
    }) for p in USER_AGENT_PARSERS],
    "os_parsers": [clean({
        "regex": p.user_agent_re.pattern, "os_replacement": p.os_replacement, "os_v1_replacement": p.os_v1_replacement,
        "os_v2_replacement": p.os_v2_replacement, "os_v3_replacement": p.os_v3_replacement,
        "os_v4_replacement": p.os_v4_replacement,
    }) for p in OS_PARSERS],
    "device_parsers": [clean({
        "regex": p.user_agent_re.pattern, "regex_flag": "i" if p.user_agent_re.flags & re.IGNORECASE else None,
        "device_replacement": p.device_replacement, "brand_replacement": p.brand_replacement,
        "model_replacement": p.model_replacement,
    }) for p in DEVICE_PARSERS],
}, indent=1, ensure_ascii=False))
