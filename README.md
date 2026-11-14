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
- Our own tables are prefixed `bsrs_` (the job queue and login sessions); Django ignores them.
- Users and passwords are upstream's (`auth_user`, Django's PBKDF2 hashes), so accounts work on both sides. Login
  sessions are not shared: after switching, everyone logs in again.
- On startup the database's `django_migrations` are checked against 2.6.3: a database that is behind is refused (upgrade
  it with upstream first), as is one migrated by a newer upstream unless `--allow-newer-upstream-schema` is given.

Not covered: SQLite- or MySQL-backed upstream installs (migrate the data to Postgres first). Upstream's file-based event
storage (`FILE_EVENT_STORAGE_PATH`) is not read yet.

### What is verified against upstream

`tools/parity/` runs the official `bugsink/bugsink:2.6.3` image next to bugsink-rs (needs docker):

- `grouping.sh`: grouping keys, issue titles and denormalized fields for ~400 edge-case events, compared with
  upstream's own functions. Grouping keys must match byte-for-byte, or new events would open duplicate issues.
- `digest.sh`: a scripted sequence of ~200 events, interleaved with issue actions that each side applies with its own
  code (resolve, also by next release and in a release; reopen; mute, also for a time and until a volume of events;
  unmute; delete, incl. the background deletion of the issue's rows; on single issues and in bulk), with vacuuming
  (`vacuum`: deleted issues, eventless issue tags, unused tag values and keys) and with settings changes (grouping-mechanism upgrade, retention changes that force eviction incl. of `never_evict` events and zero
  retention), digested four ways: upstream only, bugsink-rs only, bugsink-rs then upstream on the same database, and
  upstream then bugsink-rs. The four databases are compared row by row, and so is every alert sent along the way: emails
  (by the members' notification preferences) and messaging webhooks go to a capture server (`capture.py`) and must be
  identical. All of it is done once with default settings and once with tight quotas (`MAX_EVENTS_PER_*`,
  `MAX_EMAILS_PER_MONTH`). Telegram (whose API URL is fixed) is covered by unit tests against upstream's output instead.
- `ui_api.sh`: bugsink-rs creates a superuser and upstream checks it against its own `CREATE_SUPERUSER`; upstream
  sets up users and projects in every access situation, and bugsink-rs's UI API must allow exactly what upstream's
  views allow (project visibility, membership access, inactive users), with login, logout and CSRF checks. Passwords
  that bugsink-rs rehashes at login must still work upstream, and a password change upstream must end bugsink-rs's
  sessions. Then the issue list tabs, sorts, search, issue actions (single, project and global bulk), the issue pages
  and comments (who may edit and delete what) end to end,
  on events sent to our own ingest running behind a (simulated) proxy: the client address comes from
  `X-Forwarded-For`, and a header with more addresses than proxies gets a 400.
- `reads.sh`: what the UI API reads. The digest steps (plus a busy issue) are replayed into a database; upstream
  builds ~2,000 inputs from it and its own functions must give the same answers as ours on that database: search
  (tag terms quoted every way, tag pairs, tags plus text, LIKE wildcards, parser edge cases incl. unclosed quotes,
  escapes and Unicode whitespace) and the issue pages (header, tag summaries at both cut-offs incl. "other" and mostly
  unique keys, groupings, history, first/prev/next/last within searches, event counts, sparkline counts), and how
  every stored event is shown: what upstream's own event views (stack trace, details, breadcrumbs) hand to their
  templates, for the digest events, the grouping corpus and a display corpus of odd shapes (threads, the SDK's `_meta`
  trimming annotations incl. malformed ones, sourcemap images, log entries). Events on which upstream's view fails
  (a 500; we show what we can) are counted, not compared.
- `user_agents.sh`: the browser/OS/device contexts (and so tags) derived from a request's User-Agent header, for the
  ~30,000 user agents in uap-core's test suite plus malformed inputs, compared with upstream's own function. The
  parser data (`bugsink-server/src/digest/ua_regexes.json`) is exported from the upstream image.

### Status

Ported and verified: ingest (envelope/store, quotas and 429s) and digest (grouping, issues, releases, regressions,
time- and volume-based unmute, tags incl. User-Agent-derived ones, hourly counts, eviction and retention), alerts by
email and messaging services, login with upstream's users and passwords (sessions, CSRF, membership checks on the UI
API), issue actions (single and bulk, incl. deletion), the issue lists' tabs and search, the issue pages (tags,
history and comments, groupings, event navigation, sparkline counts, events shown as upstream shows them), and the
database bootstrap/version check. bugsink-rs can take over ingestion and digestion of an upstream
installation today; viewing and managing issues still needs upstream's UI (not at the same time, see below).

### Not yet ported (upstream behaviour missing)

In order of importance for production use (code-level gaps also have a `TODO(parity)` where they belong):

1. **The UI's API.** `/api/v1` has login, logout and the current user; the project and global issue lists (tabs,
   sorts, search, sparklines) with issue actions; the issue pages (header, tags, history and comments, groupings,
   events with navigation, search and upstream's display shaping, hourly counts). Missing:
   - accounts: signup, email confirmation, password reset, preferences, change password
   - applying uploaded sourcemaps to stack frames (frames only get the debug id hint) and the details tab's list of
     sourcemap images; the issue's Markdown export
   - projects, teams and memberships: create/edit/delete, invitations, roles, alert settings; messaging service
     setup and test messages; SDK setup pages
   - installation: users admin, auth tokens, settings and counts pages
2. **Frontend**: not started (`frontend/` holds only the plan; framework not chosen).
3. **Background jobs** besides digest, alerts, zero-retention deletion and issue deletion: deferred deletion of
   projects (`delete_project_deps`); invitation, signup confirmation, password reset and welcome emails; phone-home.
4. **Upstream's REST API** (`/api/canonical/0/`, with auth tokens) for scripts and integrations.
5. **Ingest extras**: minidumps, sourcemap and debug-file uploads (`chunk-upload`, `artifactbundle/assemble`,
   `difs/assemble`) and applying them to stack traces, CSP reports (`/security/`), `VALIDATE_ON_DIGEST`, file-based
   event storage (eviction deletes such events' rows but not their files).
6. **Settings**: `MAX_RETENTION_PER_PROJECT_EVENT_COUNT`, `TIME_ZONE` (dates in alerts are UTC).
7. **Operations**: upstream's `/health/ready` (we have `/api/v1/health`) and its other management commands
   (`vacuum`'s files, old-events, ingest-dir and orphaned-issues parts, `delete_old_events`, `cleanup_*`,
   `make_consistent`, `create_auth_token`, ...); we have `migrate`, `create-project`, `create-superuser`, `vacuum`
   (deleted issues, eventless issue tags, tags), `serve` and `worker`.

Known deliberate differences: an envelope whose event payload is not valid JSON gets a 400 at ingest (upstream returns
200 and drops the event during digest). A search of only whitespace (or only terms the parser skips) matches
everything (upstream answers 500). An event page for an event whose payload upstream's views can't handle (e.g. frames
that aren't a list of objects, malformed sourcemap images) shows what can be shown (upstream answers 500).

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
  src/api.rs           /api/v1/... JSON API for the frontend, with upstream's access rules (api/: issue pages)
  src/issues           issue actions (resolve, mute, delete, ...; single and bulk), deleting issues' rows, and what
                       the issue pages show (pages.rs)
  src/search.rs        issue and event search: query parser, tag and text matching
  src/event_display.rs an event as upstream's event pages show it (stack traces, details, breadcrumbs)
  src/proxy.rs         the client's address behind a reverse proxy (X-Real-IP / X-Forwarded-For)
  src/vacuum.rs        `bugsink vacuum`: cleanup of eventless issue tags, unused tags, deleted issues
  src/accounts         users (upstream's auth_user, Django's password hashes), login sessions, CSRF
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
echo 'a-password' | cargo run -- create-superuser you@example.com   # to log in with (unless the DB has users)
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

Behind a reverse proxy, set upstream's `BEHIND_HTTPS_PROXY` (or `BEHIND_PLAIN_HTTP_PROXY`): the client's address is
then taken from `X-Real-IP`, or with `USE_X_REAL_IP=false USE_X_FORWARDED_FOR=true X_FORWARDED_FOR_PROXY_COUNT=<n>`
from `X-Forwarded-For` (exactly n addresses expected). Without it every event gets the proxy's address.

Run `bugsink vacuum` regularly (e.g. daily from cron, like upstream's `bugsink-manage vacuum`): evicted events leave
issue tags and tag values behind, which only it cleans up.

`serve` binds to localhost by default; set `BUGSINK_BIND=0.0.0.0:8000` to expose it. `BASE_URL` also decides the UI
API's CSRF origin check (requests from another origin are refused) and, when it is https, makes the session cookie
`Secure`.

## Upgrading the upstream version

1. Bump the image in `tools/parity/*.sh`. Regenerate `bugsink-db/upstream/` from a fresh `bugsink-manage migrate` on
   Postgres (`pg_dump --schema-only`; `--data-only --inserts` for `django_migrations`, `django_content_type` and
   `auth_permission`; the sorted `django_migrations` list).
2. Diff upstream's `ingest/`, `issues/`, `events/`, `releases/`, `tags/`, `alerts/`, `phonehome/`, `sentry/`, `users/`
   and `projects/` (access rules), and Django's password hasher, between the versions and port the changes.
   Re-export the User-Agent data with `tools/parity/export_ua_regexes.py`.
3. Run all parity scripts.
