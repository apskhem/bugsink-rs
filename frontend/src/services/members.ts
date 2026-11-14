import { makeApi, Zodios } from "@zodios/core";
import { z } from "zod";

import { http } from "./http";
import { Me } from "./session";

/**
 * Members of a team or a project: the same endpoints under `/teams/{id}` and `/projects/{id}` (`kind`); `id` is the
 * team's UUID or the project's number.
 */
export type GroupKind = "teams" | "projects";

export const Member = z.object({
  user_id: z.number(),
  email: z.string(),
  username: z.string(),
  /** False until a newly invited user has set a password. */
  is_active: z.boolean(),
  role: z.number(),
  accepted: z.boolean(),
  /** Null: as per the team's or the user's setting. */
  send_email_alerts: z.boolean().nullable()
});
export type Member = z.infer<typeof Member>;

export const MemberList = z.object({
  members: z.array(Member),
  /** Whether invitations are emailed (re-send them) or passed on by hand (copy their links). */
  email_delivers: z.boolean()
});
export type MemberList = z.infer<typeof MemberList>;

export const Invited = z.object({
  user_id: z.number(),
  /** False when the address was invited before. */
  membership_created: z.boolean(),
  /** An invitation email is on its way. */
  emailed: z.boolean(),
  /** Without email: the link to pass on, if there is one to give. */
  invite_link: z.string().nullable()
});
export type Invited = z.infer<typeof Invited>;

export const MemberUpdate = z.object({
  role: z.number().optional(),
  /** `null` sets "as per the team's or user's setting"; leave it out to keep it. */
  send_email_alerts: z.boolean().nullable().optional()
});
export type MemberUpdate = z.infer<typeof MemberUpdate>;

const endpoints = makeApi([
  { method: "get", path: "/:kind/:id/members", alias: "members", response: MemberList },
  {
    method: "post",
    path: "/:kind/:id/members",
    alias: "invite",
    parameters: [{ name: "body", type: "Body", schema: z.object({ email: z.string(), role: z.number() }) }],
    response: Invited
  },
  {
    method: "patch",
    path: "/:kind/:id/members/:userId",
    alias: "updateMember",
    parameters: [{ name: "body", type: "Body", schema: MemberUpdate }],
    response: z.unknown()
  },
  { method: "delete", path: "/:kind/:id/members/:userId", alias: "removeMember", response: z.unknown() },
  {
    method: "post",
    path: "/:kind/:id/members/:userId/reinvite",
    alias: "reinvite",
    response: z.object({ email: z.string() })
  },
  {
    method: "get",
    path: "/:kind/:id/members/:userId/invite-link",
    alias: "inviteLink",
    response: z.object({ invite_link: z.string() })
  },
  { method: "post", path: "/:kind/:id/accept", alias: "accept", response: z.unknown() },
  { method: "post", path: "/:kind/:id/decline", alias: "decline", response: z.unknown() },
  {
    method: "post",
    path: "/:kind/:id/members/accept-new-user",
    alias: "acceptNewUser",
    parameters: [{ name: "body", type: "Body", schema: z.object({ token: z.string() }) }],
    response: z.object({ needs_password: z.boolean() })
  },
  {
    method: "post",
    path: "/auth/reset-password/:token",
    alias: "setPassword",
    parameters: [{ name: "body", type: "Body", schema: z.object({ password: z.string() }) }],
    response: Me
  }
]);

export const membersApi = new Zodios("/api/v1", endpoints, { axiosInstance: http });
