import { createBrowserRouter, RouterProvider } from "react-router";

import { NotFound } from "common/components/ErrorPage";
import RequireAuth from "common/components/RequireAuth";
import BareLayout from "common/layouts/BareLayout";
import BarestLayout from "common/layouts/BarestLayout";
import BaseLayout from "common/layouts/BaseLayout";
import * as accounts from "modules/accounts/routes";
import * as installation from "modules/installation/routes";
import * as issue from "modules/issue/routes";
import * as issues from "modules/issues/routes";
import * as projects from "modules/projects/routes";
import * as projectsetup from "modules/projectsetup/routes";
import * as teams from "modules/teams/routes";

const router = createBrowserRouter([
  {
    element: <BarestLayout />,
    children: [...accounts.barestRoutes, ...teams.barestRoutes, ...projects.barestRoutes]
  },
  {
    element: <RequireAuth />,
    children: [
      {
        element: <BaseLayout />,
        children: [
          ...installation.baseRoutes,
          ...accounts.baseRoutes,
          ...teams.baseRoutes,
          ...projects.baseRoutes,
          ...projectsetup.baseRoutes,
          ...issues.baseRoutes,
          ...issue.baseRoutes
        ]
      }
    ]
  },
  {
    element: <BareLayout />,
    children: [{ path: "*", element: <NotFound /> }]
  }
]);

export default function Router() {
  return <RouterProvider router={router} />;
}
