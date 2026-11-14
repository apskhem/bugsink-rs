import { useEffect, useRef } from "react";
import { Link } from "react-router";

import { routes } from "common/constants/routes";
import { useTitle } from "common/hooks/useTitle";
import { useSession } from "contexts/session";
import { sessionApi } from "services/session";

import AccountBox from "../components/AccountBox";

/**
 * Upstream's `users/logged_out.html`. The navigation logs out before coming here; visiting this page with a session
 * (Django's `LogoutView` on GET, before Django 5) logs out too.
 */
export default function LoggedOut() {
  useTitle("Logged out");
  const { user, setUser } = useSession();
  const loggingOut = useRef(false);

  useEffect(() => {
    if (user && !loggingOut.current) {
      loggingOut.current = true;
      void sessionApi.logout(undefined).then(() => setUser(null));
    }
  }, [user, setUser]);

  return (
    <AccountBox>
      You have been logged out.{" "}
      <Link to={routes.login()} className="text-cyan-500 dark:text-cyan-300">
        Log in again
      </Link>
      .
    </AccountBox>
  );
}
