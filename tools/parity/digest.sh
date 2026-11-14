#!/usr/bin/env bash
# Drop-in check at the database level. Replays the same digest steps four ways and diffs the resulting databases:
#   A: upstream only   B: bugsink-rs only
#   C: bugsink-rs bootstraps the database and digests the first half, upstream migrates it and digests the rest
#   D: upstream creates the database and digests the first half, bugsink-rs digests the rest
# Done once per settings configuration (see `configs`). Alerts (emails and messaging webhooks) go to a capture server
# (capture.py) and are compared too. Needs docker; starts its own Postgres and capture server.
# Usage: tools/parity/digest.sh [image]  (default: bugsink/bugsink:2.6.3)
set -euo pipefail
cd "$(dirname "$0")/../.."
image="${1:-bugsink/bugsink:2.6.3}"
out="$(mktemp -d)"
net=bsrs-parity pg=bsrs-parity-pg port=55499 capture=bsrs-parity-capture

# Settings for every configuration. @...@ is the capture server's address as each side sees it (see `for_side`).
common=(
    BASE_URL=https://bugsink.example.com/ "SITE_TITLE=Kim's<Sink>"
    EMAIL_HOST=@SMTP_HOST@ EMAIL_PORT=@SMTP_PORT@ EMAIL_USE_TLS=false
    MAX_EMAILS_PER_HOUR=None MAX_EMAILS_PER_MONTH=90 ALERTS_WEBHOOK_DENY_NON_GLOBAL=false
)
# Each configuration adds settings (environment variables, later ones win), given to both sides.
configs=(
    "default:"
    # 5-minute quota trips on every event, the hourly one regularly, the monthly one once; installation-wide hourly
    # too; the email quota runs out halfway
    "quotas:MAX_EVENTS_PER_PROJECT_PER_5_MINUTES=1 MAX_EVENTS_PER_PROJECT_PER_HOUR=5 MAX_EVENTS_PER_PROJECT_PER_MONTH=60 MAX_EVENTS_PER_HOUR=7 MAX_EMAILS_PER_MONTH=40"
    # every project's retention capped below what it is set to (and an installation-wide cap, which only forms check)
    "retention_cap:MAX_RETENTION_PER_PROJECT_EVENT_COUNT=30 MAX_RETENTION_EVENT_COUNT=40"
)

# upstream runs in a container on $net and reaches the capture server by name; bugsink-rs runs on the host.
for_side() { # side; text on stdin
    case "$1" in
        upstream) sed -e 's/@CAPTURE_HTTP@/capture:8025/g' -e 's/@SMTP_HOST@/capture/g' -e 's/@SMTP_PORT@/2525/g' ;;
        ours) sed -e 's/@CAPTURE_HTTP@/127.0.0.1:58025/g' -e 's/@SMTP_HOST@/127.0.0.1/g' -e 's/@SMTP_PORT@/52525/g' ;;
    esac
}

docker network create "$net" >/dev/null 2>&1 || true
docker rm -f "$pg" "$capture" >/dev/null 2>&1 || true
# No autovacuum: statistics changing mid-run change query plans, and with them how ties are broken (e.g. which of
# two equally evictable events goes first; upstream leaves that to the plan too).
docker run -d --rm --name "$pg" --network "$net" -p "$port:5432" -e POSTGRES_PASSWORD=pw postgres:17 \
    -c autovacuum=off >/dev/null
cp tools/parity/capture.py "$out/"  # docker may not be allowed to mount from the checkout's location
docker run -d --rm --name "$capture" --network "$net" --network-alias capture -p 58025:8025 -p 52525:2525 \
    -v "$out:/out" --entrypoint python "$image" /out/capture.py >/dev/null
trap 'docker rm -f "$pg" "$capture" >/dev/null' EXIT
until docker exec "$pg" pg_isready -q -U postgres; do sleep 1; done

python3 tools/parity/make_digest_steps.py >/dev/null
steps=tools/parity/digest-steps.jsonl
half=$(( $(wc -l < "$steps") / 2 ))
cargo build -q -p bugsink-server --example replay

settings=()
upstream() { # db, extra docker args...; steps on stdin
    local db="$1"; shift
    local envs=()
    while IFS= read -r kv; do envs+=(-e "$kv"); done < <(printf '%s\n' "${settings[@]}" | for_side upstream)
    docker run --rm -i --network "$net" -e SECRET_KEY=parity -e "DATABASE_URL=postgres://postgres:pw@$pg:5432/$db" \
        "${envs[@]}" "$image" "$@"
}
upstream_migrate() { upstream "$1" bugsink-manage migrate --no-input | grep -E "No migrations|Applying" | tail -1; }
upstream_digest() {
    for_side upstream | upstream "$1" bugsink-manage shell -c "$(cat tools/parity/upstream_digest.py)" | grep -v "objects imported" || true
}
ours() {
    local envs=()
    while IFS= read -r kv; do envs+=("$kv"); done < <(printf '%s\n' "${settings[@]}" | for_side ours)
    for_side ours | env "${envs[@]}" DATABASE_URL="postgres://postgres:pw@localhost:$port/$1" ./target/debug/examples/replay
}
# Between halves: the messaging services' URLs were written for the other side.
retarget() { # db, side that continues
    local from to
    [ "$2" = upstream ] && { from=127.0.0.1:58025; to=capture:8025; } || { from=capture:8025; to=127.0.0.1:58025; }
    docker exec "$pg" psql -U postgres -d "$1" -qc "UPDATE alerts_messagingserviceconfig SET config = replace(config, '$from', '$to')"
}
# What the capture server received during the previous run, set aside under the run's name.
take_capture() { touch "$out/capture.jsonl"; python3 tools/parity/normalize_capture.py < "$out/capture.jsonl" > "$out/$1.capture"; rm "$out/capture.jsonl"; }

status=0
for config in "${configs[@]}"; do
    name="${config%%:*}"
    read -r -a extra <<< "${config#*:}"
    settings=("${common[@]}" ${extra[@]+"${extra[@]}"})
    echo "=== $name ${extra[*]+${extra[*]}}"
    for db in a b c d; do docker exec "$pg" dropdb -U postgres --if-exists "$db" 2>/dev/null; docker exec "$pg" createdb -U postgres "$db"; done
    rm -f "$out/capture.jsonl"

    echo "A: upstream";                  upstream_migrate a; upstream_digest a < "$steps"; take_capture "$name-a"
    echo "B: bugsink-rs";                ours b < "$steps"; take_capture "$name-b"
    echo "C: bugsink-rs, then upstream"; head -n "$half" "$steps" | ours c; upstream_migrate c; retarget c upstream
                                         tail -n "+$((half + 1))" "$steps" | upstream_digest c; take_capture "$name-c"
    echo "D: upstream, then bugsink-rs"; upstream_migrate d; head -n "$half" "$steps" | upstream_digest d; retarget d ours
                                         tail -n "+$((half + 1))" "$steps" | ours d; take_capture "$name-d"

    for db in a b c d; do
        docker exec -i "$pg" psql -U postgres -d "$db" -At -F ' | ' -f - < tools/parity/normalize.sql > "$out/$name-$db.txt"
    done
    echo "rows compared: $(wc -l < "$out/$name-a.txt"), alerts sent: $(wc -l < "$out/$name-a.capture")"
    for db in b c d; do
        diff "$out/$name-a.txt" "$out/$name-$db.txt" > "$out/$name-a-$db.diff" || true
        diff "$out/$name-a.capture" "$out/$name-$db.capture" > "$out/$name-a-$db.capture.diff" || true
        if [ -s "$out/$name-a-$db.diff" ] || [ -s "$out/$name-a-$db.capture.diff" ]; then
            echo "A != $db:"; head -40 "$out/$name-a-$db.diff" "$out/$name-a-$db.capture.diff"; status=1
        else
            echo "A == $db"
        fi
    done
done
echo "outputs in $out"
exit $status
