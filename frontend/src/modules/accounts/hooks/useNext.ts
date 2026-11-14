import { useSearchParams } from "react-router";

import { routes } from "common/constants/routes";

/**
 * `?next=` when it is safe (Django's `url_has_allowed_host_and_scheme`: a path on this site, not `//host`), else
 * null.
 */
export function useNext(): string | null {
  const [searchParams] = useSearchParams();
  const next = searchParams.get("next");
  return next && next.startsWith("/") && !next.startsWith("//") && !next.startsWith("/\\") ? next : null;
}

/** Goes to `next` (or home): client routes via `navigate`, the backend's own pages (under `/api/`) by loading them. */
export function goTo(navigate: (to: string) => unknown, next: string | null) {
  const to = next ?? routes.home();
  if (to.startsWith("/api/")) {
    window.location.assign(to);
  } else {
    void navigate(to);
  }
}
