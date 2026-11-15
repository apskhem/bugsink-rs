import { makeApi, Zodios } from "@zodios/core";
import { z } from "zod";

import { http } from "./http";

/** Upstream's `ProjectVisibility`. */
export const PROJECT_VISIBILITIES = [
  { value: 1, label: "Joinable" },
  { value: 10, label: "Discoverable" },
  { value: 99, label: "Team Members" }
] as const;

/** Upstream's `GROUPING_MECHANISM_CHOICES`. */
export const GROUPING_MECHANISMS = [
  { value: "bugsink-v1", label: "Original, default until v2.4.0 (July 2026)" },
  { value: "bugsink-v2", label: "Value-normalized (latest)" }
] as const;

export const ProjectListItem = z.object({
  id: z.number(),
  name: z.string(),
  slug: z.string(),
  team_id: z.string().nullable(),
  team_name: z.string().nullable(),
  visibility: z.number(),
  digested_event_count: z.number(),
  stored_event_count: z.number(),
  issue_count: z.number(),
  member_count: z.number(),
  /** Not counted for projects with more than 25,000 issues (null). */
  open_issue_count: z.number().nullable(),
  my_role: z.number().nullable(),
  my_accepted: z.boolean().nullable(),
  my_send_email_alerts: z.boolean().nullable(),
  /** Through your team, or open to anyone. */
  joinable: z.boolean(),
  /** Events in each of the last 24 hours, the current one last. */
  sparkline: z.array(z.number())
});
export type ProjectListItem = z.infer<typeof ProjectListItem>;

export const ProjectTab = z.enum(["mine", "teams", "other"]);
export type ProjectTab = z.infer<typeof ProjectTab>;

export const ProjectList = z.object({
  tab: ProjectTab,
  can_create: z.boolean(),
  items: z.array(ProjectListItem)
});
export type ProjectList = z.infer<typeof ProjectList>;

/** What the new-project form offers. */
export const NewProjectOptions = z.object({
  teams: z.array(z.object({ id: z.string(), name: z.string() })),
  retention_max_event_count: z.number(),
  grouping_mechanism: z.string()
});
export type NewProjectOptions = z.infer<typeof NewProjectOptions>;

/** `project_edit`'s form: everything editable, plus the read-only slug and DSN. */
export const ProjectSettings = z.object({
  id: z.number(),
  team_id: z.string().nullable(),
  name: z.string(),
  slug: z.string(),
  dsn: z.string(),
  visibility: z.number(),
  retention_max_event_count: z.number(),
  grouping_mechanism: z.string(),
  /** While switching grouping mechanisms (30 days). */
  previous_grouping_mechanism: z.string().nullable(),
  grouping_transition_ends_at: z.string().nullable(),
  alert_on_new_issue: z.boolean(),
  alert_on_regression: z.boolean(),
  alert_on_unmute: z.boolean()
});
export type ProjectSettings = z.infer<typeof ProjectSettings>;

export const ProjectForm = z.object({
  /** On creation only. */
  team_id: z.string().optional(),
  name: z.string(),
  visibility: z.number(),
  retention_max_event_count: z.number().optional(),
  grouping_mechanism: z.string()
});
export type ProjectForm = z.infer<typeof ProjectForm>;

/** `GET /projects/{id}/summary`: what pages about a project show of it, for anyone with access to its issues. */
export const ProjectSummary = z.object({
  id: z.number(),
  name: z.string(),
  slug: z.string(),
  team_id: z.string().nullable(),
  team_name: z.string().nullable(),
  has_releases: z.boolean(),
  digested_event_count: z.number(),
  my_role: z.number().nullable(),
  is_admin: z.boolean(),
  latest_release: z.string().nullable(),
  /** Banners atop its issue list and issue pages (its ingest quota). */
  warnings: z.array(z.string())
});
export type ProjectSummary = z.infer<typeof ProjectSummary>;

const endpoints = makeApi([
  {
    method: "get",
    path: "/projects",
    alias: "projectList",
    parameters: [{ name: "tab", type: "Query", schema: ProjectTab.optional() }],
    response: ProjectList
  },
  { method: "get", path: "/projects/new", alias: "newProjectOptions", response: NewProjectOptions },
  {
    method: "post",
    path: "/projects",
    alias: "createProject",
    parameters: [{ name: "body", type: "Body", schema: ProjectForm }],
    response: z.object({ id: z.number() })
  },
  { method: "get", path: "/projects/:projectId", alias: "projectSettings", response: ProjectSettings },
  { method: "get", path: "/projects/:projectId/summary", alias: "projectSummary", response: ProjectSummary },
  {
    method: "patch",
    path: "/projects/:projectId",
    alias: "editProject",
    parameters: [{ name: "body", type: "Body", schema: ProjectForm }],
    response: z.unknown()
  },
  { method: "delete", path: "/projects/:projectId", alias: "deleteProject", response: z.unknown() },
  { method: "post", path: "/projects/:projectId/join", alias: "joinProject", response: z.unknown() },
  { method: "post", path: "/projects/:projectId/leave", alias: "leaveProject", response: z.unknown() }
]);

export const projectsApi = new Zodios("/api/v1", endpoints, { axiosInstance: http });

/** Your membership of a project, from the "mine" tab of the project list (invitations too); null without one. */
export async function myProject(projectId: number): Promise<ProjectListItem | null> {
  const { items } = await projectsApi.projectList({ queries: { tab: "mine" } });
  return items.find((project) => project.id === projectId) ?? null;
}
