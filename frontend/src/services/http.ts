import axios, { isAxiosError } from "axios";
import { z } from "zod";

/** The axios instance every API client shares: `/api/v1`, the session cookie, and the CSRF header. */
export const http = axios.create({ baseURL: "/api/v1", withCredentials: true });

let csrfToken: string | null = null;

/** From `GET /me` or login; kept in memory only (after a reload, `/me` gives it again). */
export function setCsrfToken(token: string | null) {
  csrfToken = token;
}

http.interceptors.request.use((config) => {
  const method = (config.method ?? "get").toLowerCase();
  if (csrfToken && !["get", "head", "options"].includes(method)) {
    config.headers.set("X-CSRFToken", csrfToken);
  }
  return config;
});

/** A form's errors by field (`__all__` for the form as a whole), with Django's messages. */
export type FieldErrors = Record<string, string[]>;

const ApiErrorBody = z.object({
  detail: z.string(),
  errors: z.record(z.array(z.string())).optional()
});

/** The status and body of a failed API request (null for anything else, e.g. a network error). */
export function apiError(error: unknown): { status: number; detail: string; errors: FieldErrors } | null {
  if (!isAxiosError(error) || !error.response) {
    return null;
  }
  const body = ApiErrorBody.safeParse(error.response.data);
  return {
    status: error.response.status,
    detail: body.success ? body.data.detail : error.message,
    errors: body.success ? (body.data.errors ?? {}) : {}
  };
}

/** The field errors of a 400 from a form endpoint; rethrows anything else. */
export function formErrors(error: unknown): FieldErrors {
  const failed = apiError(error);
  if (failed?.status === 400) {
    return Object.keys(failed.errors).length ? failed.errors : { __all__: [failed.detail] };
  }
  throw error;
}

/** Whether the request failed with this status. */
export function isStatus(error: unknown, status: number): boolean {
  return apiError(error)?.status === status;
}
