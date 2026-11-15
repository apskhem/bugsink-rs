import { makeApi, Zodios } from "@zodios/core";
import { z } from "zod";

import { http } from "./http";

export const User = z.object({
  id: z.number(),
  username: z.string(),
  email: z.string(),
  first_name: z.string(),
  last_name: z.string(),
  is_superuser: z.boolean(),
  is_staff: z.boolean(),
  send_email_alerts: z.boolean(),
  theme_preference: z.enum(["system", "light", "dark"]),
  language: z.string()
});
export type User = z.infer<typeof User>;

export const Me = User.extend({ csrf_token: z.string() });
export type Me = z.infer<typeof Me>;

export const SiteSettings = z.object({
  signup: z.boolean(),
  site_title: z.string(),
  single_team: z.boolean()
});
export type SiteSettings = z.infer<typeof SiteSettings>;

export const SystemWarning = z.object({
  message: z.string(),
  link: z.string().nullable(),
  /** The end of the message that links to `link`. */
  link_text: z.string().nullable(),
  silenceable: z.boolean(),
  /** A message from phoning home: its key (to silence it) and its parts, shown separated by spaces. */
  remote_message: z.string().nullable(),
  parts: z.array(z.object({ text: z.string(), url: z.string() })).nullable()
});
export type SystemWarning = z.infer<typeof SystemWarning>;

export const Home = z.discriminatedUnion("go", [
  z.object({ go: z.literal("issues"), project_id: z.number() }),
  z.object({ go: z.literal("projects") }),
  z.object({ go: z.literal("teams") })
]);

const endpoints = makeApi([
  { method: "get", path: "/auth/settings", alias: "siteSettings", response: SiteSettings },
  { method: "get", path: "/me", alias: "me", response: Me },
  {
    method: "post",
    path: "/auth/login",
    alias: "login",
    parameters: [{ name: "body", type: "Body", schema: z.object({ username: z.string(), password: z.string() }) }],
    response: Me
  },
  { method: "post", path: "/auth/logout", alias: "logout", response: z.unknown() },
  { method: "get", path: "/system-warnings", alias: "systemWarnings", response: z.array(SystemWarning) },
  { method: "post", path: "/system-warnings/email/silence", alias: "silenceEmailWarning", response: z.unknown() },
  {
    method: "post",
    path: "/system-warnings/remote/:key/silence",
    alias: "silenceRemoteMessage",
    response: z.unknown()
  },
  { method: "get", path: "/home", alias: "home", response: Home }
]);

export const sessionApi = new Zodios("/api/v1", endpoints, { axiosInstance: http });
