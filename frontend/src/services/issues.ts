import { makeApi, Zodios } from "@zodios/core";
import { z } from "zod";

import { http } from "./http";

/** `IssueSummary` plus the list's 24 hourly counts (oldest first, the current hour last). */
export const IssueListItem = z.object({
  id: z.string(),
  project_id: z.number(),
  digest_order: z.number(),
  first_seen: z.string(),
  last_seen: z.string(),
  digested_event_count: z.number(),
  stored_event_count: z.number(),
  calculated_type: z.string(),
  calculated_value: z.string(),
  transaction: z.string(),
  is_resolved: z.boolean(),
  is_muted: z.boolean(),
  sparkline: z.array(z.number()),
  project_slug: z.string()
});
export type IssueListItem = z.infer<typeof IssueListItem>;

export const IssuePage = z.object({
  items: z.array(IssueListItem),
  offset: z.number(),
  limit: z.number()
});
export type IssuePage = z.infer<typeof IssuePage>;

export const BulkActionResult = z.object({ unapplied_issue_ids: z.array(z.string()) });
export type BulkActionResult = z.infer<typeof BulkActionResult>;

const listQuery = [
  { name: "state", type: "Query", schema: z.string() },
  { name: "sort", type: "Query", schema: z.string() },
  { name: "offset", type: "Query", schema: z.number() },
  { name: "limit", type: "Query", schema: z.number() }
] as const;

const BulkAction = z.object({ action: z.string(), issue_ids: z.array(z.string()) });

const endpoints = makeApi([
  {
    method: "get",
    path: "/projects/:projectId/issues",
    alias: "projectIssues",
    parameters: [...listQuery, { name: "q", type: "Query", schema: z.string().optional() }],
    response: IssuePage
  },
  {
    method: "post",
    path: "/projects/:projectId/issues/actions",
    alias: "projectIssueActions",
    parameters: [{ name: "body", type: "Body", schema: BulkAction }],
    response: BulkActionResult
  },
  { method: "get", path: "/issues", alias: "globalIssues", parameters: [...listQuery], response: IssuePage },
  {
    method: "post",
    path: "/issues/actions",
    alias: "globalIssueActions",
    parameters: [{ name: "body", type: "Body", schema: BulkAction }],
    response: BulkActionResult
  }
]);

export const issuesApi = new Zodios("/api/v1", endpoints, { axiosInstance: http });
