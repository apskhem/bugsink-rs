#!/usr/bin/env bash
# Compares the browser/OS/device contexts derived from User-Agent headers (which become tags) between upstream Bugsink
# (official image) and this port, on uap-core's test user agents. Needs docker and network access.
# Usage: tools/parity/user_agents.sh [image]   (default: bugsink/bugsink:2.6.3)
set -euo pipefail
cd "$(dirname "$0")/../.."
image="${1:-bugsink/bugsink:2.6.3}"
out="$(mktemp -d)"

docker run --rm -i --entrypoint python "$image" - < tools/parity/make_ua_corpus.py > "$out/corpus.jsonl"
docker run --rm -i -e SECRET_KEY=parity -e DATABASE_PATH=/tmp/db.sqlite3 "$image" \
    bugsink-manage shell -c "$(cat tools/parity/upstream_user_agents.py)" < "$out/corpus.jsonl" \
    | grep "^{" > "$out/upstream.jsonl"  # just the JSON lines, not the shell's banner
cargo run -q --release -p bugsink-server --example user_agent_dump < "$out/corpus.jsonl" > "$out/ours.jsonl"

python3 - "$out" <<'PY'
import json, sys
out = sys.argv[1]
corpus, upstream, ours = ([json.loads(l) for l in open(f"{out}/{n}.jsonl")] for n in ["corpus", "upstream", "ours"])
assert len(corpus) == len(upstream) == len(ours), (len(corpus), len(upstream), len(ours))
diffs = [(c, u, o) for c, u, o in zip(corpus, upstream, ours) if u != o]
print(f"{len(corpus)} user agents compared, {len(diffs)} differ")
for c, u, o in diffs[:10]:
    print(json.dumps(c["request"]), "\n  upstream:", json.dumps(u), "\n  ours:    ", json.dumps(o))
sys.exit(1 if diffs else 0)
PY
