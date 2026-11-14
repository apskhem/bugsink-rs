import { Suspense } from "react";
import { Outlet } from "react-router";

import { NavProjectProvider } from "./NavProject";
import Navigation from "./Navigation";
import SystemWarnings from "./SystemWarnings";

/** Upstream's `base.html`: the navigation, system warnings, then the page. */
export default function BaseLayout() {
  return (
    <NavProjectProvider>
      <div id="content">
        <Navigation />
        <SystemWarnings />
        <div>
          <Suspense fallback={null}>
            <Outlet />
          </Suspense>
        </div>
      </div>
    </NavProjectProvider>
  );
}
