import { Suspense } from "react";
import { Link, Outlet } from "react-router";

import { routes } from "common/constants/routes";

/** Upstream's `bare_base.html` (error pages): the logo and "Bugsink", no menu. */
export default function BareLayout() {
  return (
    <div id="content">
      <div className="flex pl-4 bg-slate-200 dark:bg-slate-800">
        <Link to={routes.home()}>
          <img src="/images/bugsink-logo.png" className="p-2 h-12 w-12 dark:hidden block" alt="Bugsink logo" />
          <img src="/images/bugsink-logo-dark.png" className="p-2 h-12 w-12 hidden dark:block" alt="Bugsink logo" />
        </Link>
        <Link to={routes.home()}>
          <div className="pt-4 pb-4 pl-2 pr-2 font-bold">Bugsink</div>
        </Link>
      </div>
      <div>
        <Suspense fallback={null}>
          <Outlet />
        </Suspense>
      </div>
    </div>
  );
}
