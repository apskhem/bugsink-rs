#!/usr/bin/env bash
# Compares grouping keys, titles and denormalized fields between upstream Bugsink (official image) and this port.
# Usage: tools/parity/grouping.sh [image]   (default: bugsink/bugsink:2.6.3)
set -euo pipefail
cd "$(dirname "$0")/../.."
image="${1:-bugsink/bugsink:2.6.3}"
out="$(mktemp -d)"

python3 tools/parity/make_grouping_corpus.py >/dev/null
docker run --rm -i -e SECRET_KEY=parity -e DATABASE_PATH=/tmp/db.sqlite3 "$image" \
    bugsink-manage shell -c "$(cat tools/parity/upstream_grouping.py)" \
    < tools/parity/grouping-corpus.jsonl > "$out/upstream.jsonl"
cargo run -q -p bugsink-server --example grouping_dump < tools/parity/grouping-corpus.jsonl > "$out/ours.jsonl"
python3 tools/parity/compare_grouping.py tools/parity/grouping-corpus.jsonl "$out/upstream.jsonl" "$out/ours.jsonl"
