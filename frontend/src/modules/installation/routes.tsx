import { lazy } from "react";
import type { RouteObject } from "react-router";

const AuthTokenCreate = lazy(() => import("./pages/AuthTokenCreate"));
const AuthTokenList = lazy(() => import("./pages/AuthTokenList"));
const Counts = lazy(() => import("./pages/Counts"));
const Home = lazy(() => import("./pages/Home"));
const Settings = lazy(() => import("./pages/Settings"));
const UserEdit = lazy(() => import("./pages/UserEdit"));
const UserList = lazy(() => import("./pages/UserList"));

/** Pages with the navigation, for logged-in users only. (React Router matches with or without the trailing slash.) */
export const baseRoutes: RouteObject[] = [
  { path: "/", element: <Home /> },
  { path: "/users/", element: <UserList /> },
  { path: "/users/:userId/edit/", element: <UserEdit /> },
  { path: "/bsmain/auth_tokens/", element: <AuthTokenList /> },
  { path: "/bsmain/auth_tokens/create/", element: <AuthTokenCreate /> },
  { path: "/settings/", element: <Settings /> },
  { path: "/counts/", element: <Counts /> }
];
