import type { ReactNode } from "react";
import { Link } from "react-router";

import { routes } from "common/constants/routes";

/** The cyan background, the centered box and the logo on top of it: upstream's account pages (`barest_base.html`). */
export default function AccountBox({ children }: { children: ReactNode }) {
  return (
    <div className="bg-cyan-100 dark:bg-cyan-900 h-screen overflow-y-scroll flex items-center justify-center">
      <div className="bg-white dark:bg-slate-900 lg:w-5/12 md:6/12 w-10/12">
        <div className="bg-slate-200 dark:bg-slate-800 absolute left-1/2 transform -translate-x-1/2 -translate-y-1/2 rounded-full p-4 md:p-8">
          <Link to={routes.home()}>
            <img
              src="/images/bugsink-logo.png"
              className="h-8 w-8 md:h-16 md:w-16 dark:hidden block"
              alt="Bugsink logo"
            />
            <img
              src="/images/bugsink-logo-dark.png"
              className="h-8 w-8 md:h-16 md:w-16 hidden dark:block"
              alt="Bugsink logo"
            />
          </Link>
        </div>

        <div className="p-12 md:pt-24 md:pl-24 md:pr-24 md:pb-16">{children}</div>
      </div>
    </div>
  );
}
