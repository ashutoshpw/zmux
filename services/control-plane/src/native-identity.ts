import type { Pool } from "pg";
import { bearerToken, hashOpaque } from "./security";
import { HttpError } from "./http";

export type NativeIdentity = {
  tokenId: string;
  userId: string;
  name: string;
  email: string;
};

export async function requireNativeIdentity(request: Request, pool: Pool): Promise<NativeIdentity> {
  const token = bearerToken(request);
  if (!token) throw new HttpError(401, "invalid_token", "a valid bearer token is required");
  const result = await pool.query<NativeIdentity>(
    `SELECT token.id AS "tokenId", token.user_id AS "userId", account.name, account.email
       FROM native_access_token token
       JOIN "session" auth_session ON auth_session.id = token.auth_session_id
       JOIN "user" account ON account.id = token.user_id
      WHERE token.token_hash = $1
        AND token.revoked_at IS NULL
        AND token.expires_at > now()
        AND auth_session."expiresAt" > now()
        AND auth_session."userId" = token.user_id`,
    [hashOpaque(token)],
  );
  const identity = result.rows[0];
  if (!identity) throw new HttpError(401, "invalid_token", "a valid bearer token is required");
  return identity;
}
