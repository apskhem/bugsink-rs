#!/usr/bin/env bash
# Accounts across both sides, on one database: bugsink-rs bootstraps it and creates a superuser, upstream checks that
# user and sets up users and projects in every access situation, bugsink-rs's UI API must allow exactly what upstream
# allows (login, sessions, CSRF, project visibility and membership access), and upstream must accept the passwords
# bugsink-rs wrote (and its password changes must end bugsink-rs's sessions). Needs docker.
# Usage: tools/parity/accounts.sh [image]  (default: bugsink/bugsink:2.6.3)
set -euo pipefail
cd "$(dirname "$0")/../.."
image="${1:-bugsink/bugsink:2.6.3}"
out="$(mktemp -d)"
net=bsrs-parity pg=bsrs-parity-accounts-pg port=55498 http=58098
db="postgres://postgres:pw@localhost:$port/accounts"

docker network create "$net" >/dev/null 2>&1 || true
docker rm -f "$pg" >/dev/null 2>&1 || true
docker run -d --rm --name "$pg" --network "$net" -p "$port:5432" -e POSTGRES_PASSWORD=pw postgres:17 >/dev/null
server=
trap 'docker rm -f "$pg" >/dev/null; [ -z "$server" ] || kill "$server" 2>/dev/null' EXIT
until docker exec "$pg" pg_isready -q -U postgres; do sleep 1; done
docker exec "$pg" createdb -U postgres accounts

upstream() { # step
    docker run --rm -i --network "$net" -e SECRET_KEY=parity -e "STEP=$1" \
        -e "DATABASE_URL=postgres://postgres:pw@$pg:5432/accounts" \
        "$image" bugsink-manage shell -c "$(cat tools/parity/upstream_accounts.py)" | grep -v "objects imported"
}

cargo build -q --release -p bugsink-server
bugsink=./target/release/bugsink
export DATABASE_URL="$db" BASE_URL="http://127.0.0.1:$http"
$bugsink migrate
echo admin-pw | $bugsink create-superuser admin@Example.COM

upstream setup | tee "$out/setup.txt" | grep -v '^EXPECT '
sed -n 's/^EXPECT //p' "$out/setup.txt" > "$out/expect.json"

$bugsink serve --bind "127.0.0.1:$http" --workers 0 --static-dir "$out" 2> "$out/serve.log" &
server=$!
until curl -fs "http://127.0.0.1:$http/api/v1/health" >/dev/null; do sleep 0.2; done

status=0
python3 tools/parity/accounts_api.py "$BASE_URL" "$out/expect.json" "$out/state.json" before || status=1
upstream verify || status=1
python3 tools/parity/accounts_api.py "$BASE_URL" "$out/expect.json" "$out/state.json" after || status=1
echo "outputs in $out"
exit $status
