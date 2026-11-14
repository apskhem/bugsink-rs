import { useCallback, useEffect, useState } from "react";
import { useLocation, useNavigate } from "react-router";
import { z } from "zod";

const Flash = z.object({ message: z.string() });

/** Navigation state that shows `message` on the next page, as upstream's `messages.success()` before a redirect. */
export function flash(message: string): { state: z.infer<typeof Flash> } {
  return { state: { message } };
}

/**
 * Upstream's `messages`: the one passed along with `flash()` (shown once: it's taken off the history entry), plus the
 * ones `add` shows on this page.
 */
export function useMessages(): { messages: string[]; add: (message: string) => void } {
  const location = useLocation();
  const navigate = useNavigate();
  const [messages, setMessages] = useState<string[]>(() => {
    const flashed = Flash.safeParse(location.state);
    return flashed.success ? [flashed.data.message] : [];
  });

  const flashed = location.state !== null && location.state !== undefined;
  useEffect(() => {
    if (flashed) {
      void navigate(
        { pathname: location.pathname, search: location.search, hash: location.hash },
        { replace: true, state: null }
      );
    }
  }, [flashed, navigate, location.pathname, location.search, location.hash]);

  const add = useCallback((message: string) => setMessages([message]), []);
  return { messages, add };
}
