"""Generates tools/parity/display-corpus.jsonl: events (one JSON payload per line) for comparing how events are shown
(reads.sh): stack traces (chained exceptions, threads, event-level stacktraces, odd shapes), the SDK's `_meta` trimming
annotations (incl. malformed ones), sourcemap debug ids, log entries, request, deployment info, breadcrumbs."""
import json


def frame(function, lineno=10, **kw):
    return {"function": function, "filename": f"{function}.py", "lineno": lineno, "in_app": True,
            "pre_context": ["a = 1", None], "context_line": f"{function}()", "post_context": ["b = 2"], **kw}


def exc(type_, value, *frames, **kw):
    return {"type": type_, "value": value, "stacktrace": {"frames": list(frames)}, **kw}


VARS = {"a": [1, 2, 3], "b": {"x": {"y": [1]}, "z": "s"}, "c": "[Filtered]", "d": 5, "e/f~g": [1]}
events = []

# chained exceptions; Python is the only platform shown oldest call first
for platform in ["python", "javascript", "native", None]:
    e = {"exception": {"values": [
        exc("KeyError", "inner", frame("one"), frame("two", vars=VARS), mechanism={"type": "generic", "handled": True}),
        exc("ValueError", "outer", frame("three"), frame("four", colno=3), mechanism={"type": "excepthook",
                                                                                     "handled": False}),
    ]}}
    if platform:
        e["platform"] = platform
    events.append(e)

# threads: the first one with frames goes first; titles and descriptions from id/name/crashed/state
THREADS = [
    {"id": 1, "name": "main", "crashed": False, "state": "WAITING"},
    {"id": 2, "name": "worker", "crashed": True, "state": "in_progress", "stacktrace": {"frames": [frame("w1"),
                                                                                                 frame("w2")]}},
    {"name": "only name", "stacktrace": {"frames": [frame("x")]}},
    {"id": 0, "crashed": None, "state": "they're_RUNNING"},
    {"id": "t-4", "name": "", "stacktrace": None},
    {},
]
for platform in ["python", "javascript"]:
    events.append({"platform": platform, "message": "threads only", "threads": {"values": THREADS}})
    events.append({"platform": platform, "threads": THREADS,
                   "exception": [exc("E", "with threads", frame("e1"), frame("e2"))]})
events.append({"exception": [exc("E", "one thread")], "threads": [THREADS[1]]})  # fewer than 2: none shown
events.append({"message": "threads without frames", "threads": [THREADS[0], THREADS[3]],
               "stacktrace": {"frames": [frame("s1"), frame("s2")]}})
events.append({"platform": "python", "message": "event-level stacktrace", "stacktrace": {"frames": [frame("s")]}})
events.append({"message": "nothing to show", "stacktrace": {"frames": []}})
events.append({"message": "thread state not a string",
               "threads": [{"state": 3, "stacktrace": {"frames": [frame("t")]}}]})
events.append({"message": "thread id types",
               "threads": [{"id": 1.5, "name": True, "stacktrace": {"frames": [frame("t")]}}, {"id": [1], "crashed": 0}]})

# odd stacktrace shapes
for stacktrace in [None, {}, {"frames": None}, {"frames": {}}, {"frames": {"k1": 1, "k2": 2}}, {"frames": "abc"},
                   {"frames": [1, 2]}, {"frames": [frame("ok"), "not a frame"]}, []]:
    for platform in ["python", "javascript"]:
        events.append({"platform": platform, "exception": [{"type": "Odd", "value": json.dumps(stacktrace),
                                                            "stacktrace": stacktrace}]})


def trimmed(meta_values, vars_=VARS, platform="python"):
    return {"platform": platform,
            "exception": {"values": [exc("T", "trimmed", frame("f0", vars=json.loads(json.dumps(vars_))),
                                         frame("f1", vars=json.loads(json.dumps(vars_))))]},
            "_meta": {"exception": {"values": meta_values}}}


def at_frames(frames_meta, i="0"):
    return {i: {"stacktrace": {"frames": frames_meta}}}


# the SDK's trimming annotations, well-formed and not
events += [
    trimmed(at_frames({"0": {"vars": {"": {"len": 9}, "a": {"": {"len": 10}}, "b": {"x": {"y": {"": {"len": 4}}}}}}})),
    trimmed(at_frames({"1": {"vars": {"c": {"": {"rem": [["!config", "s"]]}}, "e/f~g": {"": {"len": 3}}}}})),
    trimmed(at_frames({"-1": {"vars": {"a": {"5": {}, "x": {}, "1": {"": {"len": 2}}}}}})),
    trimmed(at_frames({" 1_0": {"vars": {}}, "0": {"vars": {"": {"len": 7}}}})),
    trimmed(at_frames({"0": {"vars": {"a": {"": {"len": 10}}, "": {"len": 5}, "d": {}}}})),  # fails after the wrap
    trimmed(at_frames({"0": {"vars": {"": {"len": 5}, "a": {"": {"len": 10}}, "d": {}}}})),
    trimmed(at_frames({"0": {"vars": {"b": {"x": {"": {"len": 3}}, "z": {"": {"len": 1}}, "": {"len": 2}}}}})),
    trimmed(at_frames({"0": {"vars": {"": {"len": 4.5}}}, "1": {"vars": {"": {"len": True}}}})),
    trimmed(at_frames({"0": {"vars": {"": {}}}})),
    trimmed(at_frames({"0": {"vars": {"a": []}}})),
    trimmed(at_frames({"0": {"vars": []}})),
    trimmed(at_frames({"0": {}})),
    trimmed(at_frames({"0": {"vars": {"": {"len": 3}}}, "5": {"vars": {}}})),
    trimmed(at_frames({"x": {"vars": {}}})),
    trimmed(at_frames({"0": {"vars": {"": {"len": 3}}}}, i="3")),
    trimmed(at_frames({"0": {"vars": {"": {"len": 3}}}}, i="-1")),
    trimmed(at_frames([])),
    trimmed({"0": {"stacktrace": None}}),
    trimmed({"0": []}),
    trimmed([]),
    trimmed(at_frames({"0": {"vars": {"": {"len": 6}}}}), vars_=[1, 2], platform="javascript"),
    trimmed(at_frames({"0": {"vars": {"": {"len": 3}}}}), vars_="[Filtered]"),
    trimmed(at_frames({"0": {"vars": {"": {"len": 3}}}}), vars_=7),
    {"exception": [exc("T", "meta null", frame("f", vars=VARS))], "_meta": None},
    {"exception": [exc("T", "meta list", frame("f", vars=VARS))], "_meta": {"exception": [1]}},
    {"exception": [exc("T", "meta no values", frame("f", vars=VARS))], "_meta": {"exception": {"0": {}}}},
    {"exception": [exc("T", "meta other key", frame("f", vars=VARS))], "_meta": {"request": {"": {"len": 3}}}},
    {"exception": [exc("T", "stacktrace null", frame("f", vars=VARS)), {"type": "U", "stacktrace": None}],
     "_meta": {"exception": {"values": {"1": {}, "0": at_frames({"0": {"vars": {"": {"len": 9}}}})["0"]}}}},
]

# sourcemap images: frames of those files get the debug id
DEBUG_ID = "c941d872-af1f-4f0c-a7ff-ad3d295fe153"
js_frames = [frame("bundle", abs_path="https://x.example/app.js", filename="app.js"),
             frame("other", abs_path="https://x.example/vendor.js", filename="vendor.js"),
             frame("byname", filename="lib.js")]
for images in [
    [{"type": "sourcemap", "code_file": "https://x.example/app.js", "debug_id": DEBUG_ID},
     {"type": "sourcemap", "code_file": "lib.js", "debug_id": "{urn:uuid:" + DEBUG_ID.upper() + "}"},
     {"type": "elf", "code_file": "https://x.example/vendor.js", "debug_id": DEBUG_ID}],
    [{"type": "sourcemap", "code_file": "app.js", "debug_id": DEBUG_ID.replace("-", "")}],
    [{"type": "sourcemap", "code_file": "app.js", "debug_id": "not-a-uuid"}],
    [{"code_file": "app.js", "debug_id": DEBUG_ID}],
    [{"type": "sourcemap", "code_file": None, "debug_id": DEBUG_ID}],
    [{"type": "sourcemap", "debug_id": DEBUG_ID}, "debug_id and code_file", ["x"]],
    ["has neither"], {"key": "dict"}, "string", [], None, 5,
]:
    events.append({"platform": "javascript", "exception": [exc("TypeError", "sourcemaps", *js_frames)],
                   "debug_meta": {"images": images}})
events.append({"platform": "javascript", "exception": [exc("TypeError", "debug_meta null", *js_frames)],
               "debug_meta": None})

# details: log entries, request, deployment
for extra in [
    {"logentry": {"message": "disk %s of %s", "params": ["1", 2], "formatted": "disk 1 of 2"}, "level": "warning",
     "logger": "app.disk"},
    {"logentry": {"message": "named %(x)s", "params": {"x": "y", "z": None}}, "level": None},
    {"logentry": {"formatted": "only formatted"}, "logger": ""},
    {"message": "top-level string", "level": "info"},
    {"message": {"message": "message as dict", "params": [True]}},
    {"logentry": "logentry as string"},
    {"logger": "only logger", "exception": [exc("E", "x")]},
    {"request": {"method": "POST", "url": "https://shop.example/cart?x=1",
                 "headers": {"User-Agent": "Mozilla/5.0 (X11; Linux x86_64; rv:121.0) Gecko/20100101 Firefox/121.0"}},
     "exception": [exc("E", "with request")], "release": "1.0", "environment": None, "server_name": "web-1"},
    {"request": {"url": "https://x.example/"}, "exception": [exc("E", "no method")],
     "contexts": {"browser": {"name": "Own"}, "os": {"name": "Linux"}}},
    {"request": {}, "exception": [exc("E", "empty request")]},
    {"exception": [exc("E", "handled null", mechanism={"handled": None, "type": "t"})]},
    {"exception": [exc("E", "no mechanism")], "transaction": "/checkout"},
]:
    events.append(extra)

# breadcrumbs
for breadcrumbs in [
    {"values": [{"category": "nav", "message": "to /x", "timestamp": 1793491200.5, "type": "navigation"},
                {"category": "http", "data": {"url": "/api"}}]},
    [{"message": "as a list"}],
    {"message": "a dict without values"},
    {"values": None},
    [],
]:
    events.append({"message": "with breadcrumbs", "breadcrumbs": breadcrumbs})

with open("tools/parity/display-corpus.jsonl", "w") as f:
    for event in events:
        f.write(json.dumps(event) + "\n")
print(len(events), "events")
