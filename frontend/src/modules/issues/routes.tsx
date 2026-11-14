import { lazy } from "react";
import type { RouteObject } from "react-router";

const GlobalIssueList = lazy(() => import("./pages/GlobalIssueList"));
const ProjectIssueList = lazy(() => import("./pages/ProjectIssueList"));

/** Pages with the navigation, for logged-in users only. */
export const baseRoutes: RouteObject[] = [
  // upstream's `global_issue_list_*`
  { path: "/issues/", element: <GlobalIssueList key="open" state="open" /> },
  { path: "/issues/unresolved/", element: <GlobalIssueList key="unresolved" state="unresolved" /> },
  { path: "/issues/resolved/", element: <GlobalIssueList key="resolved" state="resolved" /> },
  { path: "/issues/muted/", element: <GlobalIssueList key="muted" state="muted" /> },
  { path: "/issues/all/", element: <GlobalIssueList key="all" state="all" /> },
  // upstream's `issue_list_*` (its "unresolved" path has no trailing slash; matching ignores it either way)
  { path: "/issues/:projectId/", element: <ProjectIssueList key="open" state="open" /> },
  { path: "/issues/:projectId/unresolved", element: <ProjectIssueList key="unresolved" state="unresolved" /> },
  { path: "/issues/:projectId/resolved/", element: <ProjectIssueList key="resolved" state="resolved" /> },
  { path: "/issues/:projectId/muted/", element: <ProjectIssueList key="muted" state="muted" /> },
  { path: "/issues/:projectId/all/", element: <ProjectIssueList key="all" state="all" /> }
];
