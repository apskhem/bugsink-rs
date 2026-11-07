"""Generates tools/parity/grouping-corpus.jsonl: event payloads chosen to exercise the edge cases of upstream's
grouping (shapes, Python string semantics, truncation, normalization, parameterization, native function names)."""
import json
import random

random.seed(42)

def exc(type_=None, value=None, **extra):
    e = {}
    if type_ is not None:
        e["type"] = type_
    if value is not None:
        e["value"] = value
    e.update(extra)
    return e

def frames(*fns, **kw):
    return {"frames": [dict({"function": f}, **kw) if isinstance(f, str) else f for f in fns]}

values = [
    "plain", "", "user 17 not found", "multi\nline\nvalue", "trailing newline\n", "\nleading newline",
    "crlf\r\nline", "unicode sep\u2028next", "vt\x0bsep", "fs\x1csep", "   padded   ", "e\u0301 decomposed",
    "x" * 1030, "\u00e9" * 1030, "e\u0301" * 600, "mail foo@example.com", "GET https://example.com/a?b=1 x",
    "host api.example.com down", "ip 10.0.0.1 and ::1 and fe80::1%eth0", "id 3f2504e0-4f89-11d3-9a0c-0305e82c3301",
    "sha da39a3ee5e6b4b0d3255bfef95601890afd80709", "md5 d41d8cd98f00b204e9800998ecf8427e",
    "at Mon, 02 Jan 2006 15:04:05 MST", "at 2024-01-02T03:04:05.123Z", "at 2024-01-02 03:04:05", "at 3:04 PM",
    "Jan 02, 1999", "took 30ms and 1.5s", "addr 0xDEADbeef", "x=1.25 y=-3 z=10", "key='abc' k2=\"d e\" flag=true",
    "datetime.datetime(2024, 1, 2, 3, 4)", "v1.2.3 and 1.2.3.4", "C:\\path\\file 12", "tab\tsep 5",
    "emoji \U0001F600 7", "a=True b=False", "100% done", "-0.5 and 0.5", "nested (1, [2, {3}])",
]

events = []
for v in values:
    events.append({"exception": {"values": [exc("ValueError", v)]}})
    events.append({"message": v})
    events.append({"logentry": {"message": v, "formatted": v.upper()}})

# shapes
events += [
    {"exception": [exc("A", "1"), exc("B", "2")]},
    {"exception": []},
    {"exception": {}},
    {"exception": None},
    {"exception": {"values": []}},
    {"exception": {"values": None}},
    {"exception": exc("Bare", "dict without values")},
    {"exception": {"values": [None]}},
    {"exception": {"values": [{}]}},
    {"exception": {"values": [exc(None, "no type")]}},
    {"exception": {"values": [exc("T" * 140, "long type")]}},
    {"exception": {"values": [exc("NoValue")]}},
    {"exception": {"values": [exc("Null", None)]}},
    {"exception": {"values": [{"type": None, "value": None}]}},
    {"exception": {"values": [exc("Zero", 0)]}},
    {"exception": {"values": [exc("Num", 12.5)]}},
    {"exception": {"values": [exc("Big", 1e16)]}},
    {"exception": {"values": [exc("Bool", True)]}},
    {"exception": {"values": [exc("Dict", {"b": 1, "a": "x"})]}},
    {"exception": {"values": [exc("List", [1, "two", None])]}},
    {"exception": {"values": [exc(123, "numeric type")]}},
    {"message": {"message": "dict message %s", "formatted": "dict message 5"}},
    {"message": {"formatted": "only formatted"}},
    {"message": 42},
    {"logentry": {"message": "", "formatted": ""}, "message": "fallback string"},
    {"logentry": {"params": [1]}},
    {"logentry": {"message": 5}},
    {"message": "   "},
    {"message": "\n\nafter blanks"},
    {},
]

# transactions (v1 includes them)
for t in ["/api/users", "", None, 5, {"a": 1}]:
    e = {"exception": {"values": [exc("E", "v")]}}
    if t is not None:
        e["transaction"] = t
    events.append(e)

# fingerprints
for fp in [["{{ default }}"], ["custom"], ["{{ default }}", "tenant", 7, 1.0, 1e16, True, None, {"k": "v"}, [1, "a"]],
           [], "abc", ["{{default}}"], [" {{ default }}"], ["it's", 'say "hi"', "back\\slash", "nl\nx", "\u00e9"]]:
    events.append({"message": "fp msg 1", "fingerprint": fp})

# crash frames, synthetic exceptions, native function names
native_fns = [
    "std::vector<int>::push_back(int const&)", "unsigned int main(int, char**)", "void foo::bar() const",
    "std::basic_ostream<char>& std::operator<<(std::basic_ostream<char>&, char const*)",
    "<actix_web::pipeline::Pipeline<S, H> as core::future::Future>::poll::h9de5d4ad3fc6c4b2",
    "@objc ViewController.causeCrash(Any) -> ()", "@foo@12", "main::$_0::operator()() const",
    "+[FOO kek]", "-[NSObject foo:bar:]", "`anonymous namespace'::helper(int)", "operator()", "<redacted>",
    "ns::func<ns::Type<int>, lambda_0123456789abcdef0123456789abcdef>(int) &", "a b c", "  spaced  ",
    "{lambda()#1}::operator()", "Foo<T> as Bar>::baz", "fn()", "<T as Trait>::method",
]
for fn in native_fns:
    for platform in ["native", "python"]:
        events.append({"platform": platform, "exception": {"values": [exc("SIGSEGV", "x", mechanism={"synthetic": True},
                                                                         stacktrace=frames("start", fn))]}})
        events.append({"platform": platform, "exception": {"values": [exc("Err", "y", stacktrace=frames("start", fn))]}})

events += [
    {"exception": {"values": [exc("E", "in_app", stacktrace=frames(
        {"function": "app", "in_app": True, "filename": "a.py", "module": "a"},
        {"function": "lib", "in_app": False, "filename": "l.py"}))]}},
    {"exception": {"values": [exc("E", "synthetic no fn", mechanism={"synthetic": True},
                                  stacktrace=frames({"function": "<unknown>"}, {"function": None}))]}},
    {"exception": {"values": [exc("E", "synthetic none", mechanism={"synthetic": 1})]}},
    {"exception": {"values": [exc("E", "raw fn", stacktrace=frames({"function": "a(int)", "raw_function": "raw"}))]},
     "platform": "native"},
    {"exception": {"values": [exc("E", "frame platform", stacktrace=frames({"function": "x::y(int)", "platform": "c"}))]}},
    {"exception": {"values": [exc("E", "abs path", mechanism={"synthetic": True},
                                  stacktrace=frames({"function": "f", "abs_path": "/abs"}))]}},
    {"stacktrace": frames({"function": "top_level", "filename": "t.py"}), "message": "no exception"},
    {"threads": {"values": [{"stacktrace": frames({"function": "only_thread"})}]}, "message": "thread"},
    {"threads": [{"stacktrace": frames("t1")}, {"stacktrace": frames("t2")}], "message": "two threads"},
    {"exception": {"values": [exc("E", "empty frames", stacktrace={"frames": []})]},
     "stacktrace": frames("fallback")},
    {"exception": {"values": [exc("E", "bad frames", stacktrace={"frames": "nope"})]}},
    {"exception": {"values": [exc("E", "frames with junk", stacktrace={"frames": [1, "x", None, {}]})]}},
    {"exception": {"values": [exc("E", "empty frame dict", stacktrace={"frames": [{}]})]}},
    {"exception": {"values": [exc("E", "module only", stacktrace=frames({"module": "m", "function": "f"}))]}},
]

# randomized messages from a token pool, to shake out parameterization differences
pool = ["id", "42", "-7", "3.14", "0x1F", "user@x.io", "http://a.b/c", "a.com", "10.1.2.3", "2024-05-06", "12:30",
        "key=", "'q'", '"dq"', "true", "=false", "5ms", "2s", "\u00e9t\u00e9", "  ", "\n", "x" * 5, "(", ")", "[", "]"]
for _ in range(150):
    msg = "".join(random.choice(pool) + random.choice(["", " ", "=", ",", ":"]) for _ in range(random.randint(1, 12)))
    events.append(random.choice([{"message": msg}, {"exception": {"values": [exc("R", msg)]}}]))

with open("tools/parity/grouping-corpus.jsonl", "w") as f:
    for e in events:
        f.write(json.dumps(e) + "\n")
print(len(events), "events")
