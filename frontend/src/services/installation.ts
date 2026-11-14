import { makeApi, Zodios } from "@zodios/core";
import { z } from "zod";

import { http } from "./http";

/** A row of the users admin (`GET /users`, all users by username). */
export const UserRow = z.object({
  id: z.number(),
  username: z.string(),
  email: z.string(),
  first_name: z.string(),
  last_name: z.string(),
  is_active: z.boolean(),
  is_superuser: z.boolean(),
  is_staff: z.boolean(),
  date_joined: z.string(),
  last_login: z.string().nullable()
});
export type UserRow = z.infer<typeof UserRow>;

export const AuthToken = z.object({
  id: z.number(),
  token: z.string(),
  description: z.string(),
  created_at: z.string(),
  is_user_bound: z.boolean(),
  /** Null once the user is deleted (or for a service token). */
  user: z.object({ id: z.number(), username: z.string() }).nullable(),
  is_project_bound: z.boolean(),
  project: z.object({ id: z.number(), name: z.string().nullable() }).nullable(),
  /** In upstream's `CAPABILITIES` order. */
  capabilities: z.array(z.string()),
  has_all_capabilities: z.boolean()
});
export type AuthToken = z.infer<typeof AuthToken>;

/** What the new-token form offers. */
export const NewAuthToken = z.object({
  /** (capability, description) */
  capabilities: z.array(z.tuple([z.string(), z.string()])),
  /** (id, name, team name) */
  projects: z.array(z.tuple([z.number(), z.string(), z.string().nullable()])),
  /** Superusers only; everyone else's tokens are always personal. */
  can_choose_user_bound: z.boolean()
});
export type NewAuthToken = z.infer<typeof NewAuthToken>;

export const AuthTokenForm = z.object({
  description: z.string(),
  is_user_bound: z.boolean(),
  project_id: z.number().nullable(),
  capabilities: z.array(z.string())
});
export type AuthTokenForm = z.infer<typeof AuthTokenForm>;

/** Rows per model, per app, in upstream's order. */
export const Counts = z.record(z.record(z.number()));
export type Counts = z.infer<typeof Counts>;

/** The settings this installation runs with, under upstream's names (in order); secrets masked. */
export const InstallationSettings = z.object({
  version: z.string(),
  upstream_version: z.string(),
  settings: z.record(z.unknown())
});
export type InstallationSettings = z.infer<typeof InstallationSettings>;

const Created = z.object({ id: z.number() });

const endpoints = makeApi([
  { method: "get", path: "/users", alias: "users", response: z.array(UserRow) },
  {
    method: "patch",
    path: "/users/:userId",
    alias: "editUser",
    parameters: [{ name: "body", type: "Body", schema: z.object({ email: z.string() }) }],
    response: z.unknown()
  },
  { method: "delete", path: "/users/:userId", alias: "deleteUser", response: z.unknown() },
  { method: "post", path: "/users/:userId/activate", alias: "activateUser", response: z.unknown() },
  { method: "post", path: "/users/:userId/deactivate", alias: "deactivateUser", response: z.unknown() },
  { method: "get", path: "/auth-tokens", alias: "authTokens", response: z.array(AuthToken) },
  { method: "get", path: "/auth-tokens/new", alias: "newAuthToken", response: NewAuthToken },
  {
    method: "post",
    path: "/auth-tokens",
    alias: "createAuthToken",
    parameters: [{ name: "body", type: "Body", schema: AuthTokenForm }],
    response: Created
  },
  { method: "delete", path: "/auth-tokens/:tokenId", alias: "revokeAuthToken", response: z.unknown() },
  { method: "get", path: "/counts", alias: "counts", response: Counts },
  { method: "get", path: "/settings", alias: "settings", response: InstallationSettings }
]);

export const installationApi = new Zodios("/api/v1", endpoints, { axiosInstance: http });
