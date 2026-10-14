import { createHash, randomBytes, timingSafeEqual } from "node:crypto";

export const NATIVE_CLIENT_ID = "zmux-macos-dev";
export const NATIVE_REDIRECT_URI = "zmux-dev://oauth/callback";

export function randomOpaque(bytes = 32): string {
  return randomBytes(bytes).toString("base64url");
}

export function hashOpaque(value: string): Buffer {
  return createHash("sha256").update(value, "utf8").digest();
}

export function validPkceChallenge(value: string): boolean {
  return /^[A-Za-z0-9_-]{43}$/.test(value);
}

export function validPkceVerifier(value: string): boolean {
  return /^[A-Za-z0-9._~-]{43,128}$/.test(value);
}

export function verifyPkce(verifier: string, expectedChallenge: string): boolean {
  if (!validPkceVerifier(verifier) || !validPkceChallenge(expectedChallenge)) return false;
  const actual = createHash("sha256").update(verifier, "ascii").digest("base64url");
  return timingSafeEqual(Buffer.from(actual, "ascii"), Buffer.from(expectedChallenge, "ascii"));
}

export function validState(value: string): boolean {
  return value.length >= 1 && value.length <= 512 && !/[\u0000-\u001f\u007f]/.test(value);
}

export function validDeviceName(value: string): boolean {
  const length = [...value].length;
  return length >= 1 && length <= 80 && value.trim() === value && !/[\u0000-\u001f\u007f]/.test(value);
}

export function validPlatform(value: string): boolean {
  return /^[A-Za-z0-9][A-Za-z0-9._ -]{0,31}$/.test(value);
}

export function bearerToken(request: Request): string | null {
  const authorization = request.headers.get("authorization");
  if (!authorization) return null;
  const match = /^Bearer ([A-Za-z0-9_-]{43})$/.exec(authorization);
  return match?.[1] ?? null;
}
