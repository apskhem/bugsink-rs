import { lazy } from "react";
import type { RouteObject } from "react-router";

const AlertsSetup = lazy(() => import("./pages/AlertsSetup"));
const MessagingServiceEdit = lazy(() => import("./pages/MessagingServiceEdit"));
const MessagingServiceNew = lazy(() => import("./pages/MessagingServiceNew"));
const SdkSetup = lazy(() => import("./pages/SdkSetup"));

/** Pages with the navigation, for logged-in users only. (React Router matches with or without the trailing slash.) */
export const baseRoutes: RouteObject[] = [
  { path: "/projects/:projectId/sdk-setup/", element: <SdkSetup /> },
  { path: "/projects/:projectId/sdk-setup/:platform/", element: <SdkSetup /> },
  { path: "/projects/:projectId/alerts/", element: <AlertsSetup /> },
  { path: "/projects/:projectId/alerts/service/add/", element: <MessagingServiceNew /> },
  { path: "/projects/:projectId/alerts/service/:serviceId/edit/", element: <MessagingServiceEdit /> }
];
