import { lazy } from "react";
import type { RouteObject } from "react-router";

import { routes } from "common/constants/routes";
import AcceptNewUser from "modules/teams/components/AcceptNewUser";

const TeamEdit = lazy(() => import("./pages/TeamEdit"));
const TeamList = lazy(() => import("./pages/TeamList"));
const TeamMemberSettings = lazy(() => import("./pages/TeamMemberSettings"));
const TeamMembers = lazy(() => import("./pages/TeamMembers"));
const TeamMembersAccept = lazy(() => import("./pages/TeamMembersAccept"));
const TeamMembersInvite = lazy(() => import("./pages/TeamMembersInvite"));
const TeamNew = lazy(() => import("./pages/TeamNew"));

/** Upstream's paths with their trailing slash, and also without. */
function both(path: string, element: RouteObject["element"]): RouteObject[] {
  return [
    { path: `${path}/`, element },
    { path, element }
  ];
}

/** Pages with the navigation, for logged-in users only. */
export const baseRoutes: RouteObject[] = [
  ...both("/teams", <TeamList />),
  ...both("/teams/mine", <TeamList tab="mine" />),
  ...both("/teams/other", <TeamList tab="other" />),
  ...both("/teams/new", <TeamNew />),
  ...both("/teams/:teamId/edit", <TeamEdit />),
  ...both("/teams/:teamId/members", <TeamMembers />),
  ...both("/teams/:teamId/members/invite", <TeamMembersInvite />),
  ...both("/teams/:teamId/members/accept", <TeamMembersAccept />),
  ...both("/teams/:teamId/members/settings/:userId", <TeamMemberSettings />)
];

/** Pages without any menu (upstream's `barest_base.html`), also for logged-out users. */
export const barestRoutes: RouteObject[] = [
  ...both("/teams/:id/members/accept/:token", <AcceptNewUser kind="teams" acceptUrl={routes.teamMembersAccept} />)
];
