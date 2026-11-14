import { createContext, use, useCallback, useEffect, useMemo, useState, type ReactNode } from "react";

import { setCsrfToken, isStatus } from "services/http";
import { sessionApi, type Me, type SiteSettings } from "services/session";

type Session = {
  /** The logged-in user, or null. */
  user: Me | null;
  settings: SiteSettings;
  /** After logging in (or anything else that starts a session), or changing preferences. */
  setUser: (user: Me | null) => void;
  /** Reloads the user from `GET /me` (null when the session is gone). */
  refresh: () => Promise<void>;
};

const SessionContext = createContext<Session | null>(null);

export function useSession(): Session {
  const session = use(SessionContext);
  if (!session) {
    throw new Error("useSession outside SessionProvider");
  }
  return session;
}

/** The logged-in user, for pages that are only routed when there is one. */
export function useUser(): Me {
  const { user } = useSession();
  if (!user) {
    throw new Error("useUser without a logged-in user");
  }
  return user;
}

async function loadUser(): Promise<Me | null> {
  try {
    return await sessionApi.me();
  } catch (error) {
    if (isStatus(error, 401)) {
      return null;
    }
    throw error;
  }
}

/** Upstream's `data-theme`: the user's preference; logged out, "system". */
function applyTheme(user: Me | null) {
  document.documentElement.setAttribute("data-theme-preference", user?.theme_preference ?? "system");
  window.__applyTheme?.();
}

type Props = { children: ReactNode; fallback: ReactNode };

/** Loads the site settings and the session before rendering anything. */
export function SessionProvider({ children, fallback }: Props) {
  const [state, setState] = useState<{ user: Me | null; settings: SiteSettings } | null>(null);

  const setUser = useCallback((user: Me | null) => {
    setCsrfToken(user?.csrf_token ?? null);
    applyTheme(user);
    setState((previous) => previous && { ...previous, user });
  }, []);

  const refresh = useCallback(async () => setUser(await loadUser()), [setUser]);

  useEffect(() => {
    void Promise.all([sessionApi.siteSettings(), loadUser()]).then(([settings, user]) => {
      setCsrfToken(user?.csrf_token ?? null);
      applyTheme(user);
      // the default title (upstream's `{{ site_title }}`), before any page sets its own
      document.title = settings.site_title;
      setState({ settings, user });
    });
  }, []);

  const value = useMemo(() => state && { ...state, setUser, refresh }, [state, setUser, refresh]);
  return value ? <SessionContext value={value}>{children}</SessionContext> : fallback;
}
