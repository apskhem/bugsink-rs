import { lazy } from "react";
import type { RouteObject } from "react-router";

import type { EventTab } from "common/constants/routes";

const EventById = lazy(() => import("modules/issue/pages/EventById"));
const EventList = lazy(() => import("modules/issue/pages/EventList"));
const EventPage = lazy(() => import("modules/issue/pages/EventPage"));
const IssueGrouping = lazy(() => import("modules/issue/pages/IssueGrouping"));
const IssueHistory = lazy(() => import("modules/issue/pages/IssueHistory"));
const IssueTags = lazy(() => import("modules/issue/pages/IssueTags"));

/** Upstream's paths, with and without the trailing slash. */
const both = (path: string, element: RouteObject["element"]): RouteObject[] => [
  { path: `${path}/`, element },
  { path, element }
];

const TAB_PATHS: [EventTab, string][] = [
  ["stacktrace", ""],
  ["details", "/details"],
  ["breadcrumbs", "/breadcrumbs"]
];

const eventRoutes: RouteObject[] = TAB_PATHS.flatMap(([tab, suffix]) => [
  // a digest order, an event id, "first" or "last"
  ...both(`/issues/issue/:issueId/event/:event${suffix}`, <EventPage tab={tab} />),
  // the previous/next event from a digest order
  ...both(`/issues/issue/:issueId/event/:event/prev${suffix}`, <EventPage tab={tab} nav="prev" />),
  ...both(`/issues/issue/:issueId/event/:event/next${suffix}`, <EventPage tab={tab} nav="next" />)
]);

/** Pages with the navigation, for logged-in users only. */
export const baseRoutes: RouteObject[] = [
  ...eventRoutes,
  ...both("/issues/issue/:issueId/events", <EventList />),
  ...both("/issues/issue/:issueId/tags", <IssueTags />),
  ...both("/issues/issue/:issueId/history", <IssueHistory />),
  ...both("/issues/issue/:issueId/grouping", <IssueGrouping />),
  ...both("/issues/event/:eventId", <EventById />)
];
