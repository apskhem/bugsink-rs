-- Tables owned by bugsink-rs itself. Everything else belongs to upstream Bugsink's Django schema (see upstream/).
-- Prefixed `bsrs_` so they can never collide with a Django app's tables; Django ignores tables it doesn't know.

-- Background jobs (replaces upstream's "snappea", which on Postgres installs lives in a separate SQLite file).
CREATE TABLE bsrs_job (
    id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    kind        text NOT NULL,
    payload     json NOT NULL,                      -- json, not jsonb: carries raw event payloads, which may hold \u0000
    run_at      timestamptz NOT NULL DEFAULT now(),
    locked_at   timestamptz,
    attempts    integer NOT NULL DEFAULT 0,
    failed_at   timestamptz,                        -- set when attempts are exhausted; kept for inspection
    last_error  text
);
CREATE INDEX bsrs_job_runnable ON bsrs_job (run_at, id) WHERE failed_at IS NULL;
