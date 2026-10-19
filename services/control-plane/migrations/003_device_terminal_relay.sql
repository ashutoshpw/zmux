ALTER TABLE device
  ADD COLUMN installation_id uuid,
  ADD COLUMN device_secret_hash bytea,
  ADD COLUMN public_key bytea,
  ADD COLUMN sharing_enabled boolean NOT NULL DEFAULT false,
  ADD COLUMN sharing_epoch uuid,
  ADD CONSTRAINT device_registration_identity_check CHECK (
    (installation_id IS NULL AND device_secret_hash IS NULL AND public_key IS NULL)
    OR (
      installation_id IS NOT NULL
      AND device_secret_hash IS NOT NULL AND octet_length(device_secret_hash) = 32
      AND public_key IS NOT NULL AND octet_length(public_key) = 32
    )
  );

CREATE UNIQUE INDEX device_user_installation_unique_idx
  ON device (user_id, installation_id);

CREATE TABLE terminal_relay_request (
  id uuid PRIMARY KEY,
  user_id text NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
  sender_device_id uuid NOT NULL REFERENCES device(id) ON DELETE CASCADE,
  target_device_id uuid NOT NULL REFERENCES device(id) ON DELETE CASCADE,
  sender_token_id uuid REFERENCES native_access_token(id) ON DELETE SET NULL,
  target_epoch uuid NOT NULL,
  payload bytea NOT NULL CHECK (octet_length(payload) BETWEEN 1 AND 262500),
  response_payload bytea,
  state text NOT NULL DEFAULT 'pending'
    CHECK (state IN ('pending', 'claimed', 'executing', 'completed', 'expired')),
  expires_at timestamptz NOT NULL,
  claimed_at timestamptz,
  started_at timestamptz,
  completed_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now(),
  CHECK (sender_device_id <> target_device_id),
  CHECK (response_payload IS NULL OR octet_length(response_payload) BETWEEN 1 AND 262500),
  CHECK ((state = 'completed') = (response_payload IS NOT NULL))
);

CREATE INDEX terminal_relay_account_state_expiry_idx
  ON terminal_relay_request (user_id, state, expires_at, created_at);

CREATE INDEX terminal_relay_target_inbox_idx
  ON terminal_relay_request (target_device_id, created_at, id)
  WHERE state = 'pending';

CREATE INDEX terminal_relay_sender_status_idx
  ON terminal_relay_request (sender_device_id, id, expires_at);
