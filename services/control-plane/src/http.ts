import type { ControlPlaneConfig } from "./config";

export const MAX_BODY_BYTES = 16 * 1024;

export class HttpError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}

export type RequestContext = { ip?: string | undefined };

export function json(data: unknown, status = 200, extraHeaders?: HeadersInit): Response {
  return Response.json(data, {
    status,
    headers: {
      "cache-control": "no-store",
      "x-content-type-options": "nosniff",
      ...extraHeaders,
    },
  });
}

export async function readJsonObject(request: Request, maxBytes = MAX_BODY_BYTES): Promise<Record<string, unknown>> {
  if (!request.headers.get("content-type")?.toLowerCase().startsWith("application/json")) {
    throw new HttpError(415, "unsupported_media_type", "content-type must be application/json");
  }
  const declaredLength = Number(request.headers.get("content-length") ?? "0");
  if (Number.isFinite(declaredLength) && declaredLength > maxBytes) {
    throw new HttpError(413, "payload_too_large", `request body exceeds ${maxBytes} bytes`);
  }
  const bytes = await request.arrayBuffer();
  if (bytes.byteLength > maxBytes) throw new HttpError(413, "payload_too_large", `request body exceeds ${maxBytes} bytes`);
  let value: unknown;
  try {
    value = JSON.parse(new TextDecoder().decode(bytes));
  } catch {
    throw new HttpError(400, "invalid_json", "request body must be valid JSON");
  }
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new HttpError(400, "invalid_request", "request body must be a JSON object");
  }
  return value as Record<string, unknown>;
}

export function exactString(body: Record<string, unknown>, key: string): string {
  const value = body[key];
  if (typeof value !== "string") throw new HttpError(400, "invalid_request", `${key} must be a string`);
  return value;
}

export function enforceSameOrigin(request: Request, config: ControlPlaneConfig): void {
  const origin = request.headers.get("origin");
  if (origin && origin !== config.origin) throw new HttpError(403, "origin_rejected", "request origin is not allowed");
}

export function clientAddress(request: Request, context: RequestContext, config: ControlPlaneConfig): string {
  if (config.trustProxy) return request.headers.get("x-forwarded-for")?.split(",", 1)[0]?.trim() || context.ip || "unknown";
  if (context.ip) return context.ip;
  return "unknown";
}
