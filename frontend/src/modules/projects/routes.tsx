import { lazy } from "react";
import type { RouteObject } from "react-router";

import { routes } from "common/constants/routes";
import AcceptNewUser from "modules/teams/components/AcceptNewUser";

const ProjectEdit = lazy(() => import("./pages/ProjectEdit"));
const ProjectList = lazy(() => import("./pages/ProjectList"));
const ProjectMemberSettings = lazy(() => import("./pages/ProjectMemberSettings"));
const ProjectMembers = lazy(() => import("./pages/ProjectMembers"));
const ProjectMembersAccept = lazy(() => import("./pages/ProjectMembersAccept"));
const ProjectMembersInvite = lazy(() => import("./pages/ProjectMembersInvite"));
const ProjectNew = lazy(() => import("./pages/ProjectNew"));

/** Upstream's paths with their trailing slash, and also without. */
function both(path: string, element: RouteObject["element"]): RouteObject[] {
  return [
    { path: `${path}/`, element },
    { path, element }
  ];
}

/** Pages with the navigation, for logged-in users only. */
export const baseRoutes: RouteObject[] = [
  ...both("/projects", <ProjectList />),
  ...both("/projects/mine", <ProjectList tab="mine" />),
  ...both("/projects/teams", <ProjectList tab="teams" />),
  ...both("/projects/other", <ProjectList tab="other" />),
  ...both("/projects/new", <ProjectNew />),
  ...both("/projects/:projectId/edit", <ProjectEdit />),
  ...both("/projects/:projectId/members", <ProjectMembers />),
  ...both("/projects/:projectId/members/invite", <ProjectMembersInvite />),
  ...both("/projects/:projectId/members/accept", <ProjectMembersAccept />),
  ...both("/projects/:projectId/members/settings/:userId", <ProjectMemberSettings />)
];

/** Pages without any menu (upstream's `barest_base.html`), also for logged-out users. */
export const barestRoutes: RouteObject[] = [
  ...both(
    "/projects/:id/members/accept/:token",
    <AcceptNewUser kind="projects" acceptUrl={(id) => routes.projectMembersAccept(Number(id))} />
  )
];
