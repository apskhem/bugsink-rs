#!/usr/bin/env bash
# Team, project and membership management through upstream's views and through our API: the same scenario
# (make_admin_steps.py) on two databases set up alike, once with email set up (invitations emailed to a capture
# server) and once without (invite links shown instead). After every step the outcome must agree (ok; invalid, with
# the same form errors; forbidden; not found; refused), as must invite links, what lists show and the database
# (admin_snapshot.sql); at the end, the emails sent. Needs docker.
# Usage: tools/parity/admin.sh [image]  (default: bugsink/bugsink:2.6.3)
set -euo pipefail
cd "$(dirname "$0")/../.."
image="${1:-bugsink/bugsink:2.6.3}"
out="$(mktemp -d)"
net=bsrs-parity pg=bsrs-parity-admin-pg port=55496 capture=bsrs-parity-admin-capture http=58097
base="http://127.0.0.1:$http"

docker network create "$net" >/dev/null 2>&1 || true
docker rm -f "$pg" "$capture" >/dev/null 2>&1 || true
docker run -d --rm --name "$pg" --network "$net" -p "$port:5432" -e POSTGRES_PASSWORD=pw postgres:17 >/dev/null
cp tools/parity/capture.py tools/parity/admin_snapshot.sql "$out/"
# "captu.test:8025": a hostname URLValidator accepts (it has a TLD), as long as ours ("127.0.0.1:58027"), so that
# messages counting a URL's characters agree.
docker run -d --rm --name "$capture" --network "$net" --network-alias capture --network-alias captu.test \
    -p 52526:2525 -p 58027:8025 \
    -v "$out:/out" --entrypoint python "$image" /out/capture.py >/dev/null
server=
trap 'docker rm -f "$pg" "$capture" >/dev/null; [ -z "$server" ] || kill "$server" 2>/dev/null' EXIT
until docker exec "$pg" pg_isready -q -U postgres; do sleep 1; done

python3 tools/parity/make_admin_steps.py >/dev/null
cargo build -q --release -p bugsink-server
# The same users on both sides; passwords "pw-<name>" (cheap hashes: bugsink-rs rehashes them at login).
users_sql=$(python3 - <<'PY'
import base64, hashlib
for name, superuser in [("admin", True), ("alice", False), ("bob", False), ("carol", False)]:
    salt = "parity" + name
    digest = base64.b64encode(hashlib.pbkdf2_hmac("sha256", f"pw-{name}".encode(), salt.encode(), 1000)).decode()
    print("INSERT INTO auth_user (password, is_superuser, username, first_name, last_name, email, is_staff, is_active, "
          "date_joined, send_email_alerts, theme_preference, language) VALUES "
          f"('pbkdf2_sha256$1000${salt}${digest}', {str(superuser).lower()}, '{name}@example.org', '{name.title()}', '', "
          f"'{name}@example.org', false, true, now(), true, 'system', 'auto');")
PY
)

status=0
for mode in email links; do
    echo "=== $mode"
    email_env=()
    [ "$mode" = email ] && email_env=(EMAIL_HOST=@HOST@ EMAIL_PORT=@PORT@ EMAIL_USE_TLS=false)
    for db in "up_$mode" "ours_$mode"; do docker exec "$pg" createdb -U postgres "$db"; done
    rm -f "$out/capture.jsonl"

    # upstream
    up_env=(-e SECRET_KEY=parity -e "DATABASE_URL=postgres://postgres:pw@$pg:5432/up_$mode" -e "BASE_URL=$base"
            -e "MODE=$mode" -e "HOST=127.0.0.1:$http" -e ALERTS_WEBHOOK_DENY_NON_GLOBAL=false)
    for kv in ${email_env[@]+"${email_env[@]}"}; do up_env+=(-e "$(echo "$kv" | sed 's/@HOST@/capture/; s/@PORT@/2525/')"); done
    docker run --rm --network "$net" "${up_env[@]}" "$image" bugsink-manage migrate --no-input >/dev/null
    echo "$users_sql" | docker exec -i "$pg" psql -U postgres -d "up_$mode" -q
    docker run --rm -i --network "$net" -v "$out:/out" "${up_env[@]}" "$image" \
        bugsink-manage shell -c "$(cat tools/parity/upstream_admin.py)" < tools/parity/admin-steps.jsonl \
        | sed -n 's/^STEP //p' > "$out/upstream-$mode.jsonl"
    sleep 1; touch "$out/capture.jsonl"; mv "$out/capture.jsonl" "$out/emails-upstream-$mode.jsonl"

    # bugsink-rs
    ours_env=(DATABASE_URL="postgres://postgres:pw@localhost:$port/ours_$mode" BASE_URL="$base"
              ALERTS_WEBHOOK_DENY_NON_GLOBAL=false)
    for kv in ${email_env[@]+"${email_env[@]}"}; do ours_env+=("$(echo "$kv" | sed 's/@HOST@/127.0.0.1/; s/@PORT@/52526/')"); done
    env "${ours_env[@]}" ./target/release/bugsink migrate >/dev/null 2>&1
    echo "$users_sql" | docker exec -i "$pg" psql -U postgres -d "ours_$mode" -q
    env "${ours_env[@]}" ./target/release/bugsink serve --bind "127.0.0.1:$http" --workers 1 --static-dir "$out" \
        2> "$out/serve-$mode.log" &
    server=$!
    until curl -fs "$base/api/v1/health" >/dev/null; do sleep 0.2; done
    python3 tools/parity/admin_api.py "$base" "$pg" "ours_$mode" "$mode" 127.0.0.1:58027 < tools/parity/admin-steps.jsonl \
        | sed -n 's/^STEP //p' > "$out/ours-$mode.jsonl"
    kill "$server"; wait "$server" 2>/dev/null || true; server=
    sleep 1; touch "$out/capture.jsonl"; mv "$out/capture.jsonl" "$out/emails-ours-$mode.jsonl"

    python3 - "$out" "$mode" <<'PY' || status=1
import json, re, sys
out, mode = sys.argv[1:]
steps = {json.loads(l)["n"]: json.loads(l) for l in open("tools/parity/admin-steps.jsonl")}
up = [json.loads(l) for l in open(f"{out}/upstream-{mode}.jsonl")]
ours = [json.loads(l) for l in open(f"{out}/ours-{mode}.jsonl")]
TOKEN = re.compile(r"/accept/[A-Za-z0-9_-]{43}/")
UUID = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")
KEY = re.compile(r"//[0-9a-f]{32}@")  # a DSN's (random) project key
CAPTURE = re.compile(r"captu\.test:8025|127\.0\.0\.1:58027")  # the capture server, as each side reaches it
# A failed send's message names the address posted to: upstream the resolved IP (it pins it), ours the URL.
POSTED_TO = re.compile(r"for url: [^ |\"']+")
norm = lambda v: json.loads(POSTED_TO.sub("for url: <url>", CAPTURE.sub("<capture>", KEY.sub("//<key>@", UUID.sub(
    "<uuid>", TOKEN.sub("/accept/<token>/", json.dumps(v, ensure_ascii=False)))))))
diffs = 0
assert len(up) == len(ours), (len(up), len(ours))
for u, o in zip(up, ours):
    s = steps[u["n"]]
    label = f"{u['n']:>2} {s['as'].split('@')[0]:>12} {s['op']} " + json.dumps(
        {k: v for k, v in s.items() if k not in ("n", "as", "op", "modes")}, ensure_ascii=False)
    keys = ["outcome", "errors", "link", "read", "needs_password", "snapshot"]
    bad = [k for k in keys if norm(u[k]) != norm(o[k])]
    print(("ok   " if not bad else "DIFF ") + label + ("" if not bad else f"  ({', '.join(bad)})")
          + f"  -> {u['outcome']}")
    for k in bad:
        diffs += 1
        if k == "snapshot":
            print("       upstream only:", sorted(set(u[k]) - set(o[k])))
            print("       ours only:    ", sorted(set(o[k]) - set(u[k])))
        else:
            print("       upstream:", json.dumps(u[k], ensure_ascii=False)[:400])
            print("       ours:    ", json.dumps(o[k], ensure_ascii=False)[:400])
emails, posts = {}, {}
for side in ["upstream", "ours"]:
    rows = [json.loads(l) for l in open(f"{out}/emails-{side}-{mode}.jsonl") if l.strip()]
    emails[side] = [norm({k: r.get(k) for k in ("to", "subject", "text", "html")}) for r in rows if r["kind"] == "smtp"]
    posts[side] = sorted(json.dumps(norm({k: r.get(k) for k in ("path", "content_type", "body")}), sort_keys=True)
                         for r in rows if r["kind"] == "http")
same_posts = posts["upstream"] == posts["ours"]
print(f"test messages: {len(posts['upstream'])} upstream, {len(posts['ours'])} ours, "
      f"{'same' if same_posts else 'DIFFERENT'}")
if not same_posts:
    diffs += 1
    for a, b in zip(posts["upstream"], posts["ours"]):
        if a != b:
            print("  upstream:", a[:600])
            print("  ours:    ", b[:600])
            break
same = emails["upstream"] == emails["ours"]
print(f"emails: {len(emails['upstream'])} upstream, {len(emails['ours'])} ours, {'same' if same else 'DIFFERENT'}")
if not same:
    diffs += 1
    for a, b in zip(emails["upstream"], emails["ours"]):
        if a != b:
            print("  upstream:", json.dumps(a, ensure_ascii=False)[:600])
            print("  ours:    ", json.dumps(b, ensure_ascii=False)[:600])
            break
outcomes = {}
for u in up:
    outcomes[u["outcome"]] = outcomes.get(u["outcome"], 0) + 1
print(f"{mode}: {len(up)} steps ({outcomes}), {diffs} differences")
sys.exit(1 if diffs else 0)
PY
done
echo "outputs in $out"
exit $status
