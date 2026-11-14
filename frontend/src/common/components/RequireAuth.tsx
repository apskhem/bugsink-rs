import { Navigate, Outlet, useLocation } from "react-router";

import { routes } from "common/constants/routes";
import { useSession } from "contexts/session";

/** Upstream's `login_required` (on every page but the account ones): to the login page, and back after. */
export default function RequireAuth() {
  const { user } = useSession();
  const { pathname, search } = useLocation();
  return user ? <Outlet /> : <Navigate to={routes.login(pathname + search)} replace />;
}
