import { lazy } from "react";
import type { RouteObject } from "react-router";

const ChangePassword = lazy(() => import("./pages/ChangePassword"));
const ConfirmEmail = lazy(() => import("./pages/ConfirmEmail"));
const LoggedOut = lazy(() => import("./pages/LoggedOut"));
const Login = lazy(() => import("./pages/Login"));
const Preferences = lazy(() => import("./pages/Preferences"));
const RequestResetPassword = lazy(() => import("./pages/RequestResetPassword"));
const ResendConfirmation = lazy(() => import("./pages/ResendConfirmation"));
const ResetPassword = lazy(() => import("./pages/ResetPassword"));
const Signup = lazy(() => import("./pages/Signup"));

/** Pages with the navigation, for logged-in users only. */
export const baseRoutes: RouteObject[] = [
  { path: "/accounts/preferences/", element: <Preferences /> },
  { path: "/accounts/change-password/", element: <ChangePassword /> }
];

/** Account pages without any menu (upstream's `barest_base.html`), also for logged-out users. */
export const barestRoutes: RouteObject[] = [
  { path: "/accounts/login/", element: <Login /> },
  { path: "/accounts/logout/", element: <LoggedOut /> },
  { path: "/accounts/signup/", element: <Signup /> },
  { path: "/accounts/resend-confirmation/", element: <ResendConfirmation /> },
  { path: "/accounts/confirm-email/:token/", element: <ConfirmEmail /> },
  { path: "/accounts/request-reset-password/", element: <RequestResetPassword /> },
  { path: "/accounts/reset-password/:token/", element: <ResetPassword /> }
];
