# Frontend (CSR)

A standalone JS workspace, separate from the Cargo workspace. The backend only needs `frontend/dist/` to contain
`index.html` plus assets: `bugsink serve` serves that directory and falls back to `index.html` for any path that
is not an API route, so client-side routing works on reload.

Contract with the backend:

- Build output: `frontend/dist/` (override with `BUGSINK_STATIC_DIR`).
- Data: `/api/v1/...` JSON (see `bugsink-server/src/api.rs`), served from upstream Bugsink's own tables, so the
  frontend works the same on a database that upstream wrote. Dev server should proxy `/api` to `http://127.0.0.1:8000`.
- Do not use client routes under `/api/`: that prefix belongs to the backend (Sentry ingest + UI API).
- Auth: `POST /api/v1/auth/login` with `{"username", "password"}` sets an HttpOnly session cookie and returns the user
  with a `csrf_token`; `GET /api/v1/me` returns the same (401 when logged out, which is the cue to show the login
  page). Every POST/PUT/PATCH/DELETE must send that token as `X-CSRFToken`; `POST /api/v1/auth/logout` ends the
  session. Keep the token in memory only; after a reload, get it again from `/me`.

## Pages to port

From upstream's Django templates (83 files; listed here by page, not partial). "API" is the backend endpoint
each page needs; ✅ means it exists already.

| Page | Upstream template | API |
|---|---|---|
| Login / logout | `templates/bugsink/login.html`, `users/logged_out.html` | ✅ `POST /api/v1/auth/login`, `/logout`, `GET /me` |
| Signup, confirm email, reset password | `templates/signup.html`, `users/{resend_confirmation,request_reset_password,reset_password}.html` | `/api/v1/auth/*` |
| Preferences, change password | `users/{preferences,change_password}.html` | `/api/v1/me` |
| Project list | `projects/project_list.html` | ✅ `GET /projects` (incl. 24h sparkline) |
| Project new / edit | `projects/project_{new,edit}.html` | `POST/PATCH /projects` |
| Project members, invite, accept | `projects/project_members*.html`, `project_member_settings.html` | `/projects/{id}/members` |
| SDK setup (Python, JS, PHP, Go) | `projects/project_sdk_setup*.html` | project DSN (from project) |
| Alert/messaging setup | `projects/project_alerts_setup.html`, `project_messaging_service_{new,edit}.html` | `/projects/{id}/messaging-services` |
| Team list / new / edit / members | `teams/*.html` | `/teams`, `/teams/{id}/members` |
| **Issue list** (open/resolved/muted/all, search, bulk actions) | `issues/issue_list.html` | ✅ `GET /projects/{id}/issues?state=&sort=&q=`, bulk ✅ `POST /projects/{id}/issues/actions` |
| Global issue list | `issues/issue_list.html` | ✅ `GET /issues?state=&sort=`, bulk ✅ `POST /issues/actions` |
| Issue header | `issues/base.html` | ✅ `GET /issues/{id}` (title, friendly id, releases, mute conditions), tag summary ✅ `GET /issues/{id}/tags` |
| **Issue: event details** | `issues/event_details.html`, `_event_nav.html` | ✅ `GET /issues/{id}/events/{first,last,<digest_order>,<event id>}`, `/events/<digest_order>/{prev,next}` (all `?q=`) |
| **Issue: stacktrace** | `issues/stacktrace.html`, `_stacktrace_frames.html` | ✅ event page `display.stacktrace` (applying uploaded sourcemaps TODO) |
| Issue: breadcrumbs | `issues/breadcrumbs.html` | ✅ event page `display.breadcrumbs` |
| Issue: event list | `issues/event_list.html` | ✅ `GET /issues/{id}/events?q=` |
| Issue: tags | `issues/tags.html` | ✅ `GET /issues/{id}/tags?all=true` |
| Issue: history (turning points, comments) | `issues/history.html` | ✅ `GET /issues/{id}/history`, `POST /issues/{id}/comments`, `PATCH/DELETE /issues/{id}/comments/{id}` |
| Issue: grouping | `issues/grouping.html` | ✅ `GET /issues/{id}/groupings` |
| Issue actions (resolve, resolve in release, mute for/until, unmute, delete) | buttons across `issues/base.html` | ✅ `POST /issues/{id}/actions` |
| Sparklines | `events/_event_sparkline*.html` | ✅ lists: `sparkline` (24 hourly counts); issue: `GET /issues/{id}/counts?q=` |
| Users admin | `users/user_{list,edit}.html` | `/users` |
| Auth tokens | `bsmain/auth_token_{list,create}.html` | `/auth-tokens` |
| Settings, counts | `templates/bugsink/{settings,counts}.html` | `/settings`, `/counts` |
| Error pages | `templates/{400,403,404,500,503}.html` | n/a (client-side) |

Issue actions take upstream's action strings as `{"action": ...}` (bulk: plus `"issue_ids": [...]`): `resolve`,
`resolved_release:<version>`, `resolved_next`, `reopen`, `mute`, `mute_for:<period>,<n>,None`,
`mute_until:<period>,<n>,<volume>`, `unmute`, `delete`. Upstream's menus offer `mute_for:` day/1, week/1, month/1,
month/3 and `mute_until:` hour/1/5, hour/24/5, hour/24/100; "resolved in latest" is `resolved_release:` with the
project's latest release. An action that doesn't fit the issue's state is not an error: a single action answers
`applied: false`, a bulk action lists such issues in `unapplied_issue_ids`. List tabs (`state=`): `open` (default),
`unresolved`, `resolved`, `muted`, `all`; `sort=`: `last_seen` (default) or `events`. Search (`q=`, project issue lists
and an issue's events, not the global list) is upstream's: `key:value` tag terms (quote either side to include spaces or
colons, e.g. `browser:"Chrome 120"`; `\"` and `\\` escape) plus plain text matched case-insensitively against the
issue's or event's type and value; everything must match.

Issue pages: an event comes with `nav` (`first`/`prev`/`next`/`last` digest orders within the search, null when there
is none, and `count`); `prev`/`next` answer 404 at the ends. Tags list each key with its most frequent values (4, or 25
with `all=true`) and an `other` remainder; `pct` is of the key's events, rounded down. History is newest first; `kind`
is upstream's `TurningPointKind` (1 first seen, 2 resolved, 3 muted, 4 regressed, 5 unmuted, 6 reopened, 10 release
info added, 100 comment) with `metadata` for the details, `user` null for Bugsink itself. Comments are stripped and an
empty one is ignored; users can edit the comment on any of their own entries but only delete their comments. Hourly
counts cover the 28 days up to the current hour (`hours`, incl. evicted events; with `q=` also `matching`, stored
events only); upstream shows them in 6/12/24-hour buckets aligned on the installation's local days.

Event pages also carry `display`, the shaping upstream's event views do (rendering is the frontend's: syntax
highlighting of code context, formatting variables and dates):
- `stacktrace.entries`: the exceptions (`is_exception_stacktrace: true`), or else the threads (with `thread_title` and
  `thread_description`), or else the event's own stacktrace; already in display order: with `stack_of_plates`
  (every platform but Python) the newest call and the outermost exception come first. Frames are the SDK's, plus
  `raise_point: true` on the frame that raised, `debug_id` for files with a sourcemap that isn't applied, and
  `vars_incomplete`: items the SDK trimmed from `vars`, by JSON pointer (`""` is `vars` itself), shown by upstream as
  "<n items trimmed…>". `stacktrace.thread_stacktraces`: with exceptions, all threads (crashed one first) if at
  least two. Upstream labels the first and last frame of each exception "raise …"/"→ begin" or "try…" (handled).
- `key_info`, `logentry_info`, `deployment_info`: label/value pairs of the details tab, upstream's labels and order;
  `contexts` incl. browser and OS derived from the User-Agent; `request_repr`: "METHOD url".
- `breadcrumbs`: the list, or null.

Not ported: Django admin, `csrf_debug`, debug email previews. Email templates stay server-side (backend renders
alert/invite emails).
