#!/usr/bin/env bash
# Upstream's REST API (/api/canonical/0/) against ours, over HTTP: the digest steps (plus issues and events enough to
# paginate, and the display and grouping corpora) are replayed into a database, users, teams, projects and auth
# tokens are added (make_rest_seed.py), and two copies of it get the same requests (make_rest_steps.py): one through
# the official image's server, one through ours. Every response must agree (status, Allow, WWW-Authenticate, content
# type, and the body byte for byte, links and cursors included), and so must the database afterwards
# (rest_snapshot.sql). Ids made during the run and its timestamps are normalized. Needs docker.
# Usage: tools/parity/rest_api.sh [image]  (default: bugsink/bugsink:2.6.3)
set -euo pipefail
cd "$(dirname "$0")/../.."
image="${1:-bugsink/bugsink:2.6.3}"
out="$(mktemp -d)"
net=bsrs-parity pg=bsrs-parity-rest-pg port=55505 up=bsrs-parity-rest-up up_port=58105 ours_port=58106
started="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
# Quotas that the steps run into; teams only by superusers (personal tokens of others are refused).
settings=(PHONEHOME=false BASE_URL=http://bugsink.test MAX_RETENTION_PER_PROJECT_EVENT_COUNT=25000 MAX_RETENTION_EVENT_COUNT=60000
          TEAM_CREATION=CB_ADMINS)

docker network create "$net" >/dev/null 2>&1 || true
docker rm -f "$pg" "$up" >/dev/null 2>&1 || true
docker run -d --rm --name "$pg" --network "$net" -p "$port:5432" -e POSTGRES_PASSWORD=pw postgres:17 >/dev/null
server=
trap 'docker rm -f "$pg" "$up" >/dev/null 2>&1; [ -z "$server" ] || kill "$server" 2>/dev/null' EXIT
until docker exec "$pg" pg_isready -q -U postgres; do sleep 1; done
docker exec "$pg" createdb -U postgres seed

python3 tools/parity/make_digest_steps.py >/dev/null
python3 tools/parity/make_display_corpus.py >/dev/null
python3 tools/parity/make_grouping_corpus.py >/dev/null
python3 tools/parity/make_rest_steps.py >/dev/null
cargo build -q --release -p bugsink-server --example replay
python3 tools/parity/make_rest_seed.py "$out/seed.sql" > "$out/many.jsonl"
python3 tools/parity/make_shown_steps.py > "$out/shown.jsonl"
grep -v -e alerts_messagingserviceconfig -e 'SET retention_max_event_count' -e '"vacuum"' \
    tools/parity/digest-steps.jsonl | cat - "$out/shown.jsonl" "$out/many.jsonl" \
    | DATABASE_URL="postgres://postgres:pw@localhost:$port/seed" ./target/release/examples/replay >/dev/null
docker exec -i "$pg" psql -U postgres -d seed -q -v ON_ERROR_STOP=1 < "$out/seed.sql"
docker exec "$pg" psql -U postgres -d seed -At -c "
    SELECT id::text FROM issues_issue UNION SELECT id::text FROM events_event
    UNION SELECT event_id::text FROM events_event UNION SELECT id::text FROM releases_release
    UNION SELECT id::text FROM teams_team UNION SELECT sentry_key::text FROM projects_project" > "$out/known-uuids.txt"
for db in up ours; do docker exec "$pg" createdb -U postgres -T seed "$db"; done

# upstream (its own image, server and snappea)
up_env=(-e PORT=8000 -e SECRET_KEY=parity-parity-parity-parity-parity-parity-parity-parity
        -e "DATABASE_URL=postgres://postgres:pw@$pg:5432/up")
for kv in "${settings[@]}"; do up_env+=(-e "$kv"); done
docker run -d --rm --name "$up" --network "$net" -p "$up_port:8000" "${up_env[@]}" "$image" >/dev/null
until curl -fs -H 'Host: bugsink.test' "http://127.0.0.1:$up_port/health/ready" >/dev/null 2>&1; do sleep 1; done
python3 tools/parity/rest_api.py "http://127.0.0.1:$up_port" "$pg" up < tools/parity/rest-steps.jsonl \
    | sed -n 's/^STEP //p' > "$out/upstream.jsonl"

# bugsink-rs
cargo build -q --release -p bugsink-server
env "${settings[@]}" DATABASE_URL="postgres://postgres:pw@localhost:$port/ours" \
    ./target/release/bugsink serve --bind "127.0.0.1:$ours_port" --workers 1 --static-dir "$out" 2> "$out/serve.log" &
server=$!
until curl -fs "http://127.0.0.1:$ours_port/api/v1/health" >/dev/null; do sleep 0.2; done
python3 tools/parity/rest_api.py "http://127.0.0.1:$ours_port" "$pg" ours < tools/parity/rest-steps.jsonl \
    | sed -n 's/^STEP //p' > "$out/ours.jsonl"

# both deleted their issue in the background: wait for that, then compare the databases
for db in up ours; do
    for _ in $(seq 60); do
        deleted="$(docker exec "$pg" psql -U postgres -d "$db" -At \
            -c 'SELECT count(*) FROM issues_issue WHERE is_deleted')"
        [ "$deleted" = 0 ] && break
        sleep 1
    done
    docker exec -i "$pg" psql -U postgres -d "$db" -At -f - < tools/parity/rest_snapshot.sql > "$out/snapshot-$db.txt"
done

status=0
python3 - "$out" "$started" <<'PY' || status=1
import base64, json, re, sys, urllib.parse
from datetime import datetime, timedelta, timezone
out, started = sys.argv[1:]
started = datetime.fromisoformat(started.replace("Z", "+00:00")) - timedelta(minutes=1)
known = set(open(f"{out}/known-uuids.txt").read().split())
steps = {json.loads(l)["n"]: json.loads(l) for l in open("tools/parity/rest-steps.jsonl")}

TIMESTAMP = re.compile(r"\d{4}-\d\d-\d\d[T ]\d\d:\d\d:\d\d(?:\.\d+)?(?:Z|[+-]\d\d(?::?\d\d)?)?")
UUID = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")
KEY = re.compile(r"//([0-9a-f]{32})@")
CURSOR = re.compile(r"cursor=([A-Za-z0-9%+/=_-]*)")


def now_if_new(m):
    text = m.group(0)
    iso = text.replace(" ", "T").replace("Z", "+00:00")
    if re.search(r"[+-]\d\d$", iso):
        iso += ":00"
    try:
        t = datetime.fromisoformat(iso)
    except ValueError:
        return text
    if t.tzinfo is None:
        t = t.replace(tzinfo=timezone.utc)
    return "<now>" if t >= started else text


class Side:
    """Ids made during the run, numbered in order of appearance (per side)."""
    def __init__(self):
        self.new = {}

    def uuid(self, m):
        u = m.group(0)
        if u in known:
            return u
        return self.new.setdefault(u, f"<new-{len(self.new) + 1}>")

    def key(self, m):
        hyphenated = str(__import__("uuid").UUID(m.group(1)))
        if hyphenated in known:
            return m.group(0)
        return "//" + self.new.setdefault(hyphenated, f"<new-{len(self.new) + 1}>") + "@"

    def cursor(self, m):
        try:
            decoded = base64.b64decode(urllib.parse.unquote_plus(m.group(1))).decode()
        except Exception:
            return m.group(0)
        return "cursor=<" + TIMESTAMP.sub(now_if_new, urllib.parse.unquote_plus(decoded)) + ">"

    def norm(self, text):
        if text is None:
            return None
        text = CURSOR.sub(self.cursor, text)
        text = KEY.sub(self.key, text)
        return TIMESTAMP.sub(now_if_new, UUID.sub(self.uuid, text))


up = [json.loads(l) for l in open(f"{out}/upstream.jsonl")]
ours = [json.loads(l) for l in open(f"{out}/ours.jsonl")]
assert len(up) == len(ours) == len(steps), (len(up), len(ours), len(steps))
u_side, o_side = Side(), Side()
diffs = html = crashes = 0
statuses = {}
for u, o in zip(up, ours):
    s = steps[u["n"]]
    statuses[u["status"]] = statuses.get(u["status"], 0) + 1
    label = f"{u['n']:>4} {s['method']} {s['path']} [{s.get('token')}]"
    if "body" in s or "raw" in s:
        label += " " + json.dumps(s.get("body", s.get("raw")), ensure_ascii=False)[:120]
    upstream_html = (u["content_type"] or "").startswith("text/html") or u["body"].lstrip().startswith("<")
    if u["status"] == 500 and o["status"] == 200 and "/events/" in s["path"]:
        # upstream's stack trace renderer raises on malformed event data; ours shows what it can
        crashes += 1
        continue
    if upstream_html or u["status"] == 500:
        # upstream's error pages (HTML): only the status can agree
        html += 1
        keys = ["status"]
    else:
        keys = ["status", "content_type", "allow", "www_authenticate", "body"]
    bad = [k for k in keys if u_side.norm(str(u[k])) != o_side.norm(str(o[k]))]
    if bad:
        diffs += 1
        print(f"DIFF {label}  ({', '.join(bad)})")
        for k in bad:
            print("       upstream:", u_side.norm(str(u[k]))[:700])
            print("       ours:    ", o_side.norm(str(o[k]))[:700])

snap = {}
for side, name in [(u_side, "up"), (o_side, "ours")]:
    snap[name] = [side.norm(line) for line in open(f"{out}/snapshot-{name}.txt").read().splitlines()]
up_only = sorted(set(snap["up"]) - set(snap["ours"]))
ours_only = sorted(set(snap["ours"]) - set(snap["up"]))
if up_only or ours_only:
    diffs += 1
    print("DIFF database")
    for line in up_only[:30]:
        print("       upstream only:", line[:300])
    for line in ours_only[:30]:
        print("       ours only:    ", line[:300])
print(f"{len(up)} requests (statuses {dict(sorted(statuses.items()))}; {html} upstream error pages compared by status, "
      f"{crashes} where upstream's stack trace rendering fails), "
      f"database {len(snap['up'])} rows; {diffs} differences")
sys.exit(1 if diffs else 0)
PY
echo "outputs in $out"
exit $status
