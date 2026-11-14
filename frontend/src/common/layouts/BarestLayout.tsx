import { Suspense } from "react";
import { Outlet } from "react-router";

/** Upstream's `barest_base.html` (login, signup and the other account pages): no menu bar at all. */
export default function BarestLayout() {
  return (
    <div id="content">
      <div>
        <Suspense fallback={null}>
          <Outlet />
        </Suspense>
      </div>
    </div>
  );
}
