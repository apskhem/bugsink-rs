# bugsink-rs

A private Rust rewrite of the [Bugsink](https://github.com/bugsink/bugsink) backend: Sentry-SDK-compatible error
tracking on Postgres, serving a separately built client-side-rendered frontend.

Private because upstream's license (PolyForm Shield 1.0.0) forbids providing a product that competes with Bugsink.

## Drop-in compatibility

bugsink-rs reads and writes **upstream Bugsink's own Postgres schema**, pinned to **Bugsink 2.6.3**:

- Point it at a database of a Postgres-backed Bugsink 2.6.3 install and it continues where upstream left off.
- Point upstream at a database bugsink-rs created or wrote to and it does the same. An empty database is bootstrapped
  with the exact schema and `django_migrations` rows that `bugsink-manage migrate` produces, so upstream sees nothing
  to migrate.
- Our own tables are prefixed `bsrs_` (currently just the job queue); Django ignores them.
- On startup the database's `django_migrations` are checked against 2.6.3: a database that is behind is refused (upgrade
  it with upstream first), as is one migrated by a newer upstream unless `--allow-newer-upstream-schema` is given.

Not covered: SQLite- or MySQL-backed upstream installs (migrate the data to Postgres first). Upstream's file-based event
storage (`FILE_EVENT_STORAGE_PATH`) is not read yet.

### What is verified against upstream

`tools/parity/` runs the official `bugsink/bugsink:2.6.3` image next to bugsink-rs (needs docker):

- `grouping.sh`: grouping keys, issue titles and denormalized fields for ~400 edge-case events, compared with
  upstream's own functions. Grouping keys must match byte-for-byte, or new events would open duplicate issues.
- `digest.sh`: a scripted sequence of ~200 events plus user actions (resolve, resolve-by-next-release, resolve-in-release,
  mute for a time and until a volume of events, grouping-mechanism upgrade, retention changes that force eviction incl.
  of `never_evict` events and zero retention) digested four ways: upstream only, bugsink-rs only, bugsink-rs then upstream on the same database, and
  upstream then bugsink-rs. The four databases are compared row by row, and so is every alert sent along the way: emails
  (by the members' notification preferences) and messaging webhooks go to a capture server (`capture.py`) and must be
  identical. All of it is done once with default settings and once with tight quotas (`MAX_EVENTS_PER_*`,
  `MAX_EMAILS_PER_MONTH`). Telegram (whose API URL is fixed) is covered by unit tests against upstream's output instead.
- `user_agents.sh`: the browser/OS/device contexts (and so tags) derived from a request's User-Agent header, for the
  ~30,000 user agents in uap-core's test suite plus malformed inputs, compared with upstream's own function. The
  parser data (`bugsink-server/src/digest/ua_regexes.json`) is exported from the upstream image.

### Status

Ported and verified: ingest (envelope/store, quotas and 429s) and digest (grouping, issues, releases, regressions,
time- and volume-based unmute, tags incl. User-Agent-derived ones, hourly counts, eviction and retention), alerts by
email and messaging services, and the database bootstrap/version check. bugsink-rs can take over ingestion and
digestion of an upstream installation today; viewing and managing issues still needs upstream's UI (not at the same
time, see below).

### Not yet ported (upstream behaviour missing)

In order of importance for production use (code-level gaps also have a `TODO(parity)` where they belong):

1. **Authentication and the UI's API.** `/api/v1` is read-only (projects, issues, events) and has no authentication,
   which is why `serve` binds to localhost. Missing:
   - accounts: login/logout on upstream's session table, CSRF, signup, email confirmation, password reset,
     preferences, change password; membership/permission checks on every endpoint
   - issue actions: resolve (also by next release / in a release), mute for/until, unmute, delete; bulk actions;
     search (`q=`) and the open/resolved/muted/all tabs; tags, history (turning points, comments), groupings and
     sparkline counts per issue
   - projects, teams and memberships: create/edit/delete, invitations, roles, alert settings; messaging service
     setup and test messages; SDK setup pages
   - installation: users admin, auth tokens, settings and counts pages
2. **Frontend**: not started (`frontend/` holds only the plan; framework not chosen).
3. **Background jobs** besides digest, alerts and zero-retention deletion: deferred deletion of issues, projects and
   events (`delete_*_deps`); tag vacuuming (`vacuum_tagvalues`, `vacuum_tagkeys`, `vacuum_eventless_issuetags`);
   invitation, signup confirmation, password reset and welcome emails; phone-home.
4. **Upstream's REST API** (`/api/canonical/0/`, with auth tokens) for scripts and integrations.
5. **Ingest extras**: minidumps, sourcemap and debug-file uploads (`chunk-upload`, `artifactbundle/assemble`,
   `difs/assemble`) and applying them to stack traces, CSP reports (`/security/`), `VALIDATE_ON_DIGEST`, file-based
   event storage (eviction deletes such events' rows but not their files), `X-Forwarded-For` behind a proxy.
6. **Settings**: `MAX_RETENTION_PER_PROJECT_EVENT_COUNT`, `TIME_ZONE` (dates in alerts are UTC).
7. **Operations**: upstream's `/health/ready` (we have `/api/v1/health`) and its management commands for cleanup and
   maintenance (`vacuum*`, `delete_old_events`, `cleanup_*`, `make_consistent`, `create_auth_token`, ...); we have
   `migrate`, `create-project`, `serve` and `worker`.

Known deliberate difference: an envelope whose event payload is not valid JSON gets a 400 at ingest. Upstream returns
200 and drops the event during digest.

## Layout

```
bugsink-db/            Postgres pool; upstream 2.6.3 schema snapshot (upstream/), bootstrap and version check;
                       bsrs_* migrations
bugsink-server/        the `bugsink` binary (src/main.rs: CLI, config, HTTP, in-process workers, static files) on a
                       library with one module per concern:
  src/protocol         Sentry wire formats: auth, content-encoding, envelopes, timestamps (pure)
  src/grouping         upstream-exact grouping keys, titles, denormalized fields; Python string semantics (pure)
  src/tasks.rs         job queue on Postgres (SKIP LOCKED + LISTEN/NOTIFY), worker loop; replaces upstream's snappea
  src/digest           upstream-exact digest: issues, groupings, events, releases, regressions, unmutes, tags,
                       counts, eviction (retention.rs)
  src/ingest.rs        POST /api/{project_id}/envelope/ and /store/ (auth, quota 429s, decode, enqueue)
  src/event_counter.rs counting events over rolling windows (for quotas and volume-based unmute)
  src/quota.rs         installation/project quotas (counted in digest, enforced in both)
  src/alerts           alert emails (upstream's templates) and messaging services (Slack, Discord, ...), as jobs
  src/api.rs           /api/v1/... JSON API for the frontend
  examples/            parity drivers used by tools/parity/
frontend/              CSR app (separate JS workspace), built into frontend/dist/; see frontend/README.md
tools/parity/          comparison harnesses against the official upstream image
```

Request flow: SDK -> `ingest` (auth + parse, respond immediately) -> `bsrs_job` table -> worker -> `digest` (one
transaction, project row locked) -> rows read by `api` -> frontend.

## Running

```sh
docker compose up -d                       # Postgres 17 on :5432
cp .env.example .env && set -a && . ./.env && set +a
cargo run -- migrate                       # bootstrap (empty DB) or verify (existing Bugsink 2.6.3 DB)
cargo run -- create-project "My App"       # prints the DSN to give to a Sentry SDK
cargo run -- serve                         # HTTP on 127.0.0.1:8000 + 4 workers
```

`bugsink worker` runs workers without HTTP, for scaling digestion separately.

Ingestion quotas use upstream's environment variables and defaults: `MAX_EVENTS_PER_PROJECT_PER_5_MINUTES` (1000),
`MAX_EVENTS_PER_PROJECT_PER_HOUR` (5000), `MAX_EVENTS_PER_PROJECT_PER_MONTH` (1000000), and the installation-wide
`MAX_EVENTS_PER_5_MINUTES`, `MAX_EVENTS_PER_HOUR`, `MAX_EVENTS_PER_MONTH` (same defaults).

Alerts too: `BASE_URL` (for links), `SITE_TITLE`; email via `EMAIL_HOST`, `EMAIL_PORT`, `EMAIL_HOST_USER`,
`EMAIL_HOST_PASSWORD`, `EMAIL_USE_TLS`/`EMAIL_USE_SSL`, `EMAIL_TIMEOUT`, `DEFAULT_FROM_EMAIL`, `MAX_EMAILS_PER_HOUR`,
`MAX_EMAILS_PER_MONTH` (without `EMAIL_HOST`, emails are only logged); messaging webhooks are restricted by
`ALERTS_WEBHOOK_OUTBOUND_MODE`, `ALERTS_WEBHOOK_ALLOW_LIST`, `ALERTS_WEBHOOK_DENY_LIST` and
`ALERTS_WEBHOOK_DENY_NON_GLOBAL`. `bugsink --help` lists them all. Give `serve` and `worker` the same values.

Don't run upstream and bugsink-rs against the same database at the same time: each assumes it is the only digester.

The UI API has no authentication yet, which is why `serve` binds to localhost by default.

## Upgrading the upstream version

1. Bump the image in `tools/parity/*.sh`. Regenerate `bugsink-db/upstream/` from a fresh `bugsink-manage migrate` on
   Postgres (`pg_dump --schema-only`; `--data-only --inserts` for `django_migrations`, `django_content_type` and
   `auth_permission`; the sorted `django_migrations` list).
2. Diff upstream's `ingest/`, `issues/`, `events/`, `releases/`, `tags/`, `alerts/`, `phonehome/` and `sentry/` between
   the versions and port the changes. Re-export the User-Agent data with `tools/parity/export_ua_regexes.py`.
3. Run all parity scripts.
