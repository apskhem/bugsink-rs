#!/usr/bin/env bash
# The UI API, on one database with upstream: bugsink-rs bootstraps it and creates a superuser, upstream checks that
# user and sets up users and projects in every access situation, bugsink-rs's UI API must allow exactly what upstream
# allows (login, sessions, CSRF, project visibility and membership access), and upstream must accept the passwords
# bugsink-rs wrote (and its password changes must end bugsink-rs's sessions). Then issue lists and actions through the
# API on events sent to our ingest, which runs as if behind a proxy (X-Forwarded-For) (what the actions write is
# compared with upstream by digest.sh). Needs docker.
# Usage: tools/parity/ui_api.sh [image]  (default: bugsink/bugsink:2.6.3)
set -euo pipefail
cd "$(dirname "$0")/../.."
image="${1:-bugsink/bugsink:2.6.3}"
out="$(mktemp -d)"
net=bsrs-parity pg=bsrs-parity-ui-api-pg port=55498 http=58098
db="postgres://postgres:pw@localhost:$port/ui_api"

docker network create "$net" >/dev/null 2>&1 || true
docker rm -f "$pg" >/dev/null 2>&1 || true
docker run -d --rm --name "$pg" --network "$net" -p "$port:5432" -e POSTGRES_PASSWORD=pw postgres:17 >/dev/null
server=
trap 'docker rm -f "$pg" >/dev/null; [ -z "$server" ] || kill "$server" 2>/dev/null' EXIT
until docker exec "$pg" pg_isready -q -U postgres; do sleep 1; done
docker exec "$pg" createdb -U postgres ui_api

upstream() { # step
    docker run --rm -i --network "$net" -e SECRET_KEY=parity -e "STEP=$1" \
        -e "DATABASE_URL=postgres://postgres:pw@$pg:5432/ui_api" \
        "$image" bugsink-manage shell -c "$(cat tools/parity/upstream_ui_api.py)" | grep -v "objects imported"
}

cargo build -q --release -p bugsink-server
bugsink=./target/release/bugsink
export DATABASE_URL="$db" BASE_URL="http://127.0.0.1:$http"
$bugsink migrate
echo admin-pw | $bugsink create-superuser admin@Example.COM

upstream setup | tee "$out/setup.txt" | grep -v '^EXPECT '
sed -n 's/^EXPECT //p' "$out/setup.txt" > "$out/expect.json"

BEHIND_PLAIN_HTTP_PROXY=true USE_X_REAL_IP=false USE_X_FORWARDED_FOR=true X_FORWARDED_FOR_PROXY_COUNT=1 \
    $bugsink serve --bind "127.0.0.1:$http" --workers 1 --static-dir "$out" 2> "$out/serve.log" &
server=$!
until curl -fs "http://127.0.0.1:$http/api/v1/health" >/dev/null; do sleep 0.2; done

status=0
python3 tools/parity/ui_api.py "$BASE_URL" "$out/expect.json" "$out/state.json" before || status=1
# The client address from X-Forwarded-For; without the header (a proxy misconfiguration) there is none. (Issue "c"
# and its event were deleted by then.)
addrs=$(docker exec "$pg" psql -U postgres -d ui_api -Atc \
    "SELECT string_agg(coalesce(host(remote_addr), 'none'), ',' ORDER BY ingested_at) FROM events_event")
if [ "$addrs" = "198.51.100.7,none,none" ]; then echo "ok   remote_addr from X-Forwarded-For"
else echo "FAIL remote_addr from X-Forwarded-For: $addrs"; status=1; fi
upstream verify || status=1
python3 tools/parity/ui_api.py "$BASE_URL" "$out/expect.json" "$out/state.json" after || status=1
echo "outputs in $out"
exit $status
