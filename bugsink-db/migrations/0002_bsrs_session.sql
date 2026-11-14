-- Login sessions of the UI API. Not upstream's django_session: its rows are signed with upstream's SECRET_KEY, which
-- we don't share, so switching between upstream and bugsink-rs means logging in again.
CREATE TABLE bsrs_session (
    key_hash    bytea PRIMARY KEY,                  -- sha256 of the cookie value; the value itself is never stored
    user_id     bigint NOT NULL REFERENCES auth_user (id) ON DELETE CASCADE,
    csrf_token  text NOT NULL,
    auth_hash   bytea NOT NULL,                     -- sha256 of auth_user.password at login: a password change ends it
    created_at  timestamptz NOT NULL DEFAULT now(),
    expires_at  timestamptz NOT NULL
);
CREATE INDEX bsrs_session_user_id ON bsrs_session (user_id);
CREATE INDEX bsrs_session_expires_at ON bsrs_session (expires_at);
