# Runs inside the official Bugsink image (it has PyYAML): prints JSONL events with User-Agent headers, from uap-core's
# test cases (thousands of real-world user agents, pinned below) plus inputs of unusual shapes.
import json
import urllib.request

import yaml

UAP_CORE = "https://raw.githubusercontent.com/ua-parser/uap-core/73e7340c3ed8055051607b296bf46ead7aa5f19e/"
FILES = ["tests/test_ua.yaml", "tests/test_os.yaml", "tests/test_device.yaml", "test_resources/firefox_user_agent_strings.yaml",
         "test_resources/pgts_browser_list.yaml", "test_resources/opera_mini_user_agent_strings.yaml",
         "test_resources/podcasting_user_agent_strings.yaml"]

seen = set()
for name in FILES:
    with urllib.request.urlopen(UAP_CORE + name) as f:
        for case in yaml.safe_load(f)["test_cases"]:
            ua = case["user_agent_string"]
            if ua not in seen:
                seen.add(ua)
                print(json.dumps({"request": {"headers": {"User-Agent": ua}}}))

chrome = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.6099.71 Safari/537.36"
for event in [
    {"request": {"headers": {"User-Agent": [chrome, "second"]}}},
    {"request": {"headers": {"User-Agent": []}}},
    {"request": {"headers": {"User-Agent": None}}},
    {"request": {"headers": {"User-Agent": 42}}},
    {"request": {"headers": {"User-Agent": [None]}}},
    {"request": {"headers": {"user-agent": chrome}}},
    {"request": {"headers": [["User-Agent", chrome]]}},
    {"request": "not a dict", "contexts": {"trace": {"trace_id": "t"}}},
    {"request": {"headers": {"User-Agent": chrome}}, "contexts": {"browser": {"name": "Sent by SDK"}, "os": None}},
    {"request": {"headers": {"User-Agent": chrome}}, "contexts": "not a dict"},
    {"request": {"headers": {"User-Agent": ""}}},
    {"request": {"headers": {"User-Agent": "Foo/1² (Bar 2²)"}}},
    {"request": {"headers": {"User-Agent": "Mozilla/5.0 (Linux; Android 007.00; Pixel 7) Chrome/0120.00.1"}}},
]:
    print(json.dumps(event))
