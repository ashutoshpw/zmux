CREATE TABLE IF NOT EXISTS native_authorization_code (
  code_hash bytea PRIMARY KEY,
  user_id text NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
  auth_session_id text NOT NULL REFERENCES "session"(id) ON DELETE CASCADE,
  client_id text NOT NULL,
  redirect_uri text NOT NULL,
  code_challenge text NOT NULL,
  expires_at timestamptz NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS native_authorization_code_expiry_idx
  ON native_authorization_code (expires_at);

CREATE TABLE IF NOT EXISTS native_access_token (
  id uuid PRIMARY KEY,
  token_hash bytea NOT NULL UNIQUE,
  user_id text NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
  auth_session_id text NOT NULL REFERENCES "session"(id) ON DELETE CASCADE,
  expires_at timestamptz NOT NULL,
  revoked_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS native_access_token_user_idx
  ON native_access_token (user_id, created_at DESC);

CREATE TABLE IF NOT EXISTS device (
  id uuid PRIMARY KEY,
  user_id text NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
  name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 80),
  platform text NOT NULL CHECK (char_length(platform) BETWEEN 1 AND 32),
  last_seen_at timestamptz NOT NULL,
  revoked_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS device_user_idx ON device (user_id, created_at DESC);
