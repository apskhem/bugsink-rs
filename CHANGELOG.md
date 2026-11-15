# Changelog

Versions follow the upstream Bugsink line that bugsink-rs is a drop-in for: `MAJOR.MINOR` is upstream's, the patch
number counts bugsink-rs's own releases (see "Versions" in the README).

## Unreleased

- Much less memory: deriving browser/OS/device from User-Agent headers took ~175 MB, plus ~75 MB per thread that did
  it, for the ~1,300 compiled regexes of upstream's parser data. A regex is now only compiled for user agents that
  contain the text it requires; common browsers need a few dozen, and parsing is ~4x faster. The results are
  unchanged.

## 2.6.1 (2026-11-15)

- A database that upstream installed before it squashed its early migrations, then upgraded to 2.6.3, is no longer
  refused as "migrated by a newer Bugsink": the `django_migrations` rows of those squashed-away migrations are
  ignored, as Django does.

## 2.6.0 (2026-11-15)

The first release: a Rust backend and React frontend that can take over a Postgres-backed **Bugsink 2.6.3**
installation, and hand it back.

### Drop-in for Bugsink 2.6.3

- Reads and writes upstream's own Postgres schema. Point it at an upstream 2.6.3 database and it continues where
  upstream left off; point upstream at a database bugsink-rs created or wrote to and it does the same.
- An empty database gets exactly the schema and `django_migrations` that `bugsink-manage migrate` makes. A database
  that is behind 2.6.3 is refused; one from a newer upstream needs `--allow-newer-upstream-schema`.
- Users and passwords are upstream's (Django's PBKDF2 hashes). Login sessions are not shared: after switching,
  everyone logs in again.
- Same environment variables and defaults as upstream's Docker image (`BASE_URL`, `SITE_TITLE`, `EMAIL_*`,
  `MAX_EVENTS_*`, `SINGLE_USER`, `USER_REGISTRATION`, proxy settings, ...). `.env.example` lists every one.

### What's in it

- **Ingest:** the Sentry SDK endpoints (`/api/{project}/envelope/`, `/store/`), with installation and project quotas
  (429s) and client addresses behind a reverse proxy.
- **Digest:** grouping (keys byte-for-byte as upstream's), issues, releases, regressions, time- and volume-based
  unmute, tags incl. browser/OS from the User-Agent, hourly counts, retention and eviction. Background jobs run on a
  queue in Postgres, in `serve` or in separate `worker` processes.
- **Alerts:** by email and to messaging services (Slack, Discord, Mattermost, Microsoft Teams, Google Chat, Telegram,
  custom webhooks), per members' notification preferences.
- **The UI:** every upstream page as a React app: issue lists (tabs, sorts, search, bulk actions), issue pages
  (stack traces, event details incl. Sourcemap IDs, breadcrumbs, events, tags, grouping, history and comments),
  teams, projects and members (invitations by email or link), messaging setup, SDK setup, accounts (signup with
  email confirmation, password reset, preferences), the users admin, auth tokens, counts, settings, and the system
  and project warnings.
- **REST API:** upstream's `/api/canonical/0/` with auth tokens, its OpenAPI schema and Swagger UI.
- **Phoning home** like upstream: once an hour, bugsink.com gets upstream's message about the installation (version
  2.6.3, the upstream version; no Python version), and superusers see the notices it answers with. `PHONEHOME=false`
  turns it off; `bugsink print-phonehome` shows what is sent.
- **Commands:** `serve`, `worker`, `migrate`, `create-project`, `create-superuser` (or `CREATE_SUPERUSER` on
  `serve`), `send-welcome-email`, `vacuum`, `print-phonehome`; `/health/ready` and `/api/v1/health`.
- **Container:** the `Dockerfile` builds a static binary and the frontend into a `scratch` image.

### How it was checked

Scripts in `tools/parity/` run the official `bugsink/bugsink:2.6.3` image next to bugsink-rs and compare them:
grouping (~400 edge-case events), digest (~200 events with issue actions, retention and quota changes, digested both
ways and switching sides midway; databases compared row by row, and every alert sent), the UI API's access rules and
actions, what the issue and event pages show (~2,000 inputs), team/project/account management (~170 steps, with and
without email), the REST API (~1,000 requests, byte for byte), and User-Agent parsing (~30,000 user agents). All
pass with 0 differences.

### Not included (optional upstream extras)

Sourcemap and debug-file uploads (`sentry-cli` uploads fail, and stack traces aren't un-minified), minidumps, CSP
reports, file-based event storage, `VALIDATE_ON_DIGEST`, `TIME_ZONE` (alert emails use UTC), translations (English
only), and upstream's other management commands. Before switching, check that the installation doesn't use the
storage or uploads (all three should be 0):

```sql
SELECT count(*) FROM events_event WHERE storage_backend IS NOT NULL;
SELECT count(*) FROM files_file;
SELECT count(*) FROM files_chunk;
```

Where upstream answers with a 500 (malformed events, some invalid management actions), bugsink-rs shows what it can
or says what's wrong; the README lists these differences.

### Switching from upstream

1. Stop upstream (don't run both against the same database at once).
2. Start bugsink-rs with the same `DATABASE_URL` and settings; it checks the schema on start.
3. Run `bugsink vacuum` regularly (e.g. daily), as with upstream.

Switching back is the same in reverse.

### Requirements

PostgreSQL (tested with 17). Building: Rust 1.94+, Node `^20.19.0 || >=22.12.0`.

### License

PolyForm Shield 1.0.0, as upstream Bugsink: bugsink-rs may not be used to provide a product that competes with
Bugsink. Third-party parts are listed in `THIRD-PARTY-NOTICES.md`.
