CREATE INDEX IF NOT EXISTS native_access_token_expiry_idx
  ON native_access_token (expires_at, id);
