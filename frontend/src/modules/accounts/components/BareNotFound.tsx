import { Link } from "react-router";

import { NotFound } from "common/components/ErrorPage";
import { routes } from "common/constants/routes";
import { useTitle } from "common/hooks/useTitle";

/**
 * Upstream's `404.html` (with its `bare_base.html` header: logo and "Bugsink") for the account pages, which are
 * routed without that layout. `message` is the `Http404`'s.
 */
export default function BareNotFound({ message }: { message?: string }) {
  useTitle({ full: "404 Not Found" });
  return (
    <>
      <div className="flex pl-4 bg-slate-200 dark:bg-slate-800">
        <Link to={routes.home()}>
          <img src="/images/bugsink-logo.png" className="p-2 h-12 w-12 dark:hidden block" alt="Bugsink logo" />
          <img src="/images/bugsink-logo-dark.png" className="p-2 h-12 w-12 hidden dark:block" alt="Bugsink logo" />
        </Link>
        <Link to={routes.home()}>
          <div className="pt-4 pb-4 pl-2 pr-2 font-bold">Bugsink</div>
        </Link>
      </div>
      {message === undefined ? <NotFound /> : <NotFound message={message} />}
    </>
  );
}
