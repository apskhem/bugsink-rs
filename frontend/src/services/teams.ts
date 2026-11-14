import { makeApi, Zodios } from "@zodios/core";
import { z } from "zod";

import { http } from "./http";

/** Upstream's `TeamVisibility`. */
export const TEAM_VISIBILITIES = [
  { value: 1, label: "Joinable" },
  { value: 10, label: "Discoverable" },
  { value: 99, label: "Hidden" }
] as const;

/** Upstream's `TeamRole` (and `ProjectRole`, the same). */
export const ROLES = [
  { value: 0, label: "Member" },
  { value: 1, label: "Admin" }
] as const;

export const ROLE_ADMIN = 1;

export function roleLabel(role: number): string {
  return ROLES.find((choice) => choice.value === role)?.label ?? String(role);
}

export const TeamListItem = z.object({
  id: z.string(),
  name: z.string(),
  visibility: z.number(),
  /** Incl. projects being deleted (as upstream counts them). */
  project_count: z.number(),
  member_count: z.number(),
  /** Your membership, if any (an invitation: not accepted yet). */
  my_role: z.number().nullable(),
  my_accepted: z.boolean().nullable(),
  my_send_email_alerts: z.boolean().nullable(),
  /** Whether you can join it without an invitation (any, for superusers). */
  joinable: z.boolean()
});
export type TeamListItem = z.infer<typeof TeamListItem>;

export const TeamTab = z.enum(["mine", "other"]);
export type TeamTab = z.infer<typeof TeamTab>;

export const TeamList = z.object({
  tab: TeamTab,
  can_create: z.boolean(),
  items: z.array(TeamListItem)
});
export type TeamList = z.infer<typeof TeamList>;

export const Team = z.object({
  id: z.string(),
  name: z.string(),
  visibility: z.number()
});
export type Team = z.infer<typeof Team>;

export const TeamForm = z.object({
  name: z.string(),
  visibility: z.number()
});
export type TeamForm = z.infer<typeof TeamForm>;

const endpoints = makeApi([
  {
    method: "get",
    path: "/teams",
    alias: "teamList",
    parameters: [{ name: "tab", type: "Query", schema: TeamTab.optional() }],
    response: TeamList
  },
  {
    method: "post",
    path: "/teams",
    alias: "createTeam",
    parameters: [{ name: "body", type: "Body", schema: TeamForm }],
    response: z.object({ id: z.string() })
  },
  { method: "get", path: "/teams/:teamId", alias: "team", response: Team },
  {
    method: "patch",
    path: "/teams/:teamId",
    alias: "editTeam",
    parameters: [{ name: "body", type: "Body", schema: TeamForm }],
    response: z.unknown()
  },
  { method: "post", path: "/teams/:teamId/join", alias: "joinTeam", response: z.unknown() },
  { method: "post", path: "/teams/:teamId/leave", alias: "leaveTeam", response: z.unknown() }
]);

export const teamsApi = new Zodios("/api/v1", endpoints, { axiosInstance: http });

/** Your membership of a team, from the "mine" tab of the team list (which lists invitations too); null without one. */
export async function myTeam(teamId: string): Promise<TeamListItem | null> {
  const { items } = await teamsApi.teamList({ queries: { tab: "mine" } });
  return items.find((team) => team.id === teamId) ?? null;
}
