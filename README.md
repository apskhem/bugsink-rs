# bugsink-rs

A private Rust rewrite of the [Bugsink](https://github.com/bugsink/bugsink) backend: Sentry-SDK-compatible error
tracking on Postgres, serving a separately built client-side-rendered frontend.

Private because upstream's license (PolyForm Shield 1.0.0) forbids providing a product that competes with Bugsink.
Being a port of Bugsink, bugsink-rs is under that same license (`LICENSE`); parts from third parties are listed in
`THIRD-PARTY-NOTICES.md`.

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
  trimming annotations incl. malformed ones, sourcemap images incl. uploaded ones, log entries). Events on which
  upstream's view fails (a 500; we show what we can) are counted, not compared.
- `admin.sh`: team, project and membership management. A scenario of ~100 steps (creating, editing and deleting teams
  and projects, joining and leaving, inviting existing and new users, member settings, answering invitations, new
  users setting their password, messaging services with test messages, signing up, confirming the address, password
  resets, preferences and password changes, the users admin, auth tokens, counts, settings, system warnings (incl.
  messages from phoning home, by version, and silencing them) and where "home" goes, plus what must be refused and what lists show) goes through upstream's own views and through our API, on databases set up alike, once with email and once
  without (invite links instead, and signing up without confirming the address). After every step the outcome (incl. each form's error messages), invite links and
  a snapshot of users, teams, projects, memberships, tokens and messaging services must agree; at the end, the
  emails (invitations, confirmations, password resets) and test messages sent.
- `rest_api.sh`: upstream's REST API (`/api/canonical/0/`) over HTTP. The digest steps, a display corpus, the
  grouping corpus and issues and events enough to paginate are replayed into a database, with users, teams, projects
  and auth tokens of every kind (service, personal, project-bound, expired, of inactive users or deleted projects,
  inconsistent ones); two copies of it get the same ~1,000 requests, one through the official image's server and
  one through ours: authentication and capabilities, what personal and project-bound tokens may reach, lists with
  every filter, search, sort and order, cursors followed back and forth (ties included), malformed identifiers,
  cursors, bodies and content types, every event's payload and stack trace as Markdown, triaging, comments, history,
  releases (incl. resolving by the next one), creating and updating projects and teams, `OPTIONS` and the schema.
  Every response must agree byte for byte (status, `Allow`, `WWW-Authenticate`, content type, body incl. links and
  cursors; ids and timestamps made during the run normalized), and so must the database afterwards. Where upstream
  answers with an HTML error page only the status is compared; events on which upstream's stack trace rendering
  fails (a 500; we render what we can) are counted, not compared.
- `user_agents.sh`: the browser/OS/device contexts (and so tags) derived from a request's User-Agent header, for the
  ~30,000 user agents in uap-core's test suite plus malformed inputs, compared with upstream's own function. The
  parser data (`bugsink-server/src/ua_regexes.json`) is exported from the upstream image.

### Status

Ported and verified: ingest (envelope/store, quotas and 429s) and digest (grouping, issues, releases, regressions,
time- and volume-based unmute, tags incl. User-Agent-derived ones, hourly counts, eviction and retention), alerts by
email and messaging services, login with upstream's users and passwords (sessions, CSRF, membership checks on the UI
API), issue actions (single and bulk, incl. deletion), the issue lists' tabs and search, the issue pages (tags,
history and comments, groupings, event navigation, sparkline counts, events shown as upstream shows them), team,
project and membership management (invitations by email or link, new users setting their password, messaging
services with test messages, project deletion), accounts (signup with email confirmation, password reset,
preferences, changing the password), the installation's admin (users, auth tokens, counts, settings, system
warnings), phoning home (`PHONEHOME`) and the messages it brings back, upstream's REST API with auth tokens (`/api/canonical/0/`, its schema and Swagger UI), and the database
bootstrap/version check. bugsink-rs can take over an upstream installation: ingestion, digestion, and upstream's UI
as a client-side React app (`frontend/`) on the UI API.

### Not yet ported (upstream behaviour missing)

Nothing beyond the optional extras below. Every page is ported to the frontend (see `frontend/README.md`), checked
against the API's responses and, in a headless browser, side by side with upstream's pages on the same database.

Phoning home sends what upstream's message has, as far as it applies to bugsink-rs: the version is the upstream
version whose schema it runs on (2.6.3), the Python version is null, and the runtime (machine, Docker or not) is
bugsink-rs's. `bugsink print-phonehome` prints it; `PHONEHOME=false` turns it off.

### Skipped on purpose (optional extras)

Not planned: an installation needs these only in particular setups, and works without them otherwise. In code they
are marked `SKIPPED(parity)`.

- **Sourcemap and debug-file uploads** (`sentry-cli`'s endpoints: `/api/0/` and its `chunk-upload`,
  `artifactbundle/assemble`, `difs/assemble`) and applying them to stack traces (the event details' "Sourcemap IDs"
  do say which were uploaded, e.g. by upstream before the switch). Needed if CI runs `sentry-cli sourcemaps upload` or `debug-files upload`: those calls fail here
  (which can fail a build step), and JavaScript stack traces stay minified (frames only get the "no sourcemap for
  debug_id" hint).
- **Minidumps** (`/minidump/`, `event.minidump` attachments): native crash reports (C/C++, Electron, game engines)
  aren't processed.
- **File-based event storage** (`FILE_EVENT_STORAGE...`; upstream's default keeps events in the database): such
  events can't be shown, and eviction deletes their rows but not their files.
- **CSP reports** (`/security/`) and **`VALIDATE_ON_DIGEST`**.
- **`TIME_ZONE`**: dates in alert emails are UTC (the frontend shows times in the browser's zone).
- **Translations**: upstream shows each user its pages and form messages in their language (English or Simplified
  Chinese); the API's messages are English, and the language preference is stored only.
- **Upstream's other management commands** (`vacuum`'s files, old-events, ingest-dir and orphaned-issues parts,
  `delete_old_events`, `cleanup_*`, `make_consistent`, `create_auth_token`, ...). Retention and eviction happen
  during digest as upstream's do. We have `migrate`, `create-project`, `create-superuser`, `send-welcome-email`,
  `vacuum` (deleted issues, eventless issue tags, tags), `print-phonehome`, `serve` and `worker`; `/health/ready` (upstream's readiness
  probe) and `/api/v1/health` (which also checks the database).

To check whether an existing installation uses the storage and uploads (all three 0: it doesn't):

```sql
SELECT count(*) FROM events_event WHERE storage_backend IS NOT NULL;  -- events in file-based storage
SELECT count(*) FROM files_file;                                       -- uploaded sourcemaps and debug files
SELECT count(*) FROM files_chunk;                                      -- uploads through sentry-cli
```

Known deliberate differences: an envelope whose event payload is not valid JSON gets a 400 at ingest (upstream returns
200 and drops the event during digest). A search of only whitespace (or only terms the parser skips) matches
everything (upstream answers 500). An event page for an event whose payload upstream's views can't handle (e.g. frames
that aren't a list of objects, malformed sourcemap images) shows what can be shown (upstream answers 500). Where
upstream's management views fail with a 500 or quietly do nothing, the API answers with what's wrong: deleting a
team whose projects have any data (409; upstream's database refuses it too), a project name whose slug would exceed
50 characters, an unknown messaging service kind (400), changing your membership settings before accepting the
invitation, re-sending an invitation without email set up or asking for its link with email set up (409),
re-sending the confirmation for an address nobody signed up with (404). "Home" doesn't send you to a deleted project.
The counts page counts our job queue where upstream counts snappea's, and its backlog warning is about our queue.
In the REST API, an event whose stack trace upstream can't render as Markdown gets one anyway (upstream: a 500);
errors are JSON where upstream renders an HTML page (500s, unknown paths, errors on `/stacktrace/`); integers
beyond 64 bits in request bodies and "mute until" thresholds beyond 2^32 aren't supported.

## Layout

```
bugsink-db/            Postgres pool; upstream 2.6.3 schema snapshot (upstream/), bootstrap and version check;
                       bsrs_* migrations
bugsink-server/        the `bugsink` binary (src/main.rs: CLI, config, HTTP, in-process workers, static files) on a
                       library with one module per concern:
  src/protocol         Sentry wire formats: auth, content-encoding, envelopes, timestamps (pure)
  src/py.rs            Python and Django semantics everything else relies on: str(), repr(), json.dumps, strip,
                       truncatechars, isoformat, int(), UUID() (pure)
  src/grouping         upstream-exact grouping keys, titles, denormalized fields (pure)
  src/tasks.rs         job queue on Postgres (SKIP LOCKED + LISTEN/NOTIFY), worker loop; replaces upstream's snappea
  src/digest           upstream-exact digest: issues, groupings, events, regressions, unmutes, tags, counts,
                       eviction (retention.rs)
  src/releases.rs      releases: created on first sight, ordered (semver or by date), "resolved by next release"
  src/user_agent.rs    browser/OS/device from the User-Agent header, with upstream's parser data (ua_regexes.json)
  src/ingest.rs        POST /api/{project_id}/envelope/ and /store/ (auth, quota 429s, decode, enqueue)
  src/event_counter.rs counting events over rolling windows (for quotas and volume-based unmute)
  src/quota.rs         installation/project quotas (counted in digest, enforced in both)
  src/alerts           alert emails and messaging services (Slack, Discord, ...), as jobs
  src/mail             sending email: SMTP settings, quotas, upstream's templates (the HTML ones share a layout)
  src/api.rs           /api/v1/... JSON API for the frontend, with upstream's access rules (api/: issue pages)
  src/issues           issue actions (resolve, mute, delete, ...; single and bulk), deleting issues' rows, and what
                       the issue pages show (pages.rs)
  src/search.rs        issue and event search: query parser, tag and text matching
  src/projects.rs      teams and projects: lists, forms, deleting
  src/forms.rs         Django's form validation: errors by field, its messages
  src/memberships.rs   members, invitations (email or link), joining and leaving; who may do what
  src/alerts/setup.rs  messaging services: config forms (Django's URLField/URLValidator), test messages
  src/installation.rs  users admin, auth tokens, counts, system warnings, home
  src/rest.rs          /api/canonical/0/...: upstream's REST API (bearer auth tokens, DRF's errors, fields and cursor
                       pagination in rest/; the OpenAPI schema and Swagger UI page are upstream's, in rest/schema/)
  src/event_display.rs an event as upstream's event pages show it (stack traces, details, breadcrumbs)
  src/proxy.rs         the client's address behind a reverse proxy (X-Real-IP / X-Forwarded-For)
  src/vacuum.rs        `bugsink vacuum`: cleanup of eventless issue tags, unused tags, deleted issues
  src/accounts         users (upstream's auth_user, Django's password hashes), login sessions, CSRF, signup and
                       the rest of the account pages, Django's email and password validation
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
npm install && npm run build               # the frontend, into frontend/dist/ (npm run dev: Vite on :3000)
cargo run -- serve                         # HTTP on 127.0.0.1:8000 + 4 workers, serving frontend/dist/
```

`bugsink worker` runs workers without HTTP, for scaling digestion separately.

Or as a container: the `Dockerfile` builds the frontend and a static (musl) binary into a `scratch` image that runs
`serve` on 0.0.0.0:8000 (any other command: append it to `docker run`).

```sh
docker build -t bugsink-rs .
docker run -e DATABASE_URL=postgres://... -e BASE_URL=https://bugsink.example.com -p 8000:8000 bugsink-rs
echo 'a-password' | docker run -i -e DATABASE_URL=postgres://... bugsink-rs create-superuser you@example.com
```

As upstream's image, `serve` creates the first user from `CREATE_SUPERUSER=email:password` while the database has
no users (and ignores it after), so a fresh installation needs no separate `create-superuser`.

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
from `X-Forwarded-For` (exactly n addresses expected). Without it every event gets the proxy's address. The links
the REST API returns (pagination) use `X-Forwarded-Proto` there, and `X-Forwarded-Host` with `USE_X_FORWARDED_HOST`.

Users, teams and projects use upstream's settings: `SINGLE_USER`, `SINGLE_TEAM`, `TEAM_CREATION` and
`USER_REGISTRATION` (`CB_ANYBODY`, `CB_MEMBERS`, `CB_ADMINS`, `CB_NOBODY`; signing up needs `CB_ANYBODY`),
`USER_REGISTRATION_VERIFY_EMAIL`, `USER_REGISTRATION_VERIFY_EMAIL_EXPIRY`,
and the retention caps `MAX_RETENTION_PER_PROJECT_EVENT_COUNT` (also caps eviction) and `MAX_RETENTION_EVENT_COUNT`.
Invitations are emailed when email is set up (`EMAIL_HOST`, or an `EMAIL_BACKEND` other than the console/dummy
ones); otherwise the API returns the links to pass on.

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
