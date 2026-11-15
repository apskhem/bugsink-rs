import { makeApi, Zodios } from "@zodios/core";
import { z } from "zod";

import { http } from "./http";

/** An issue as the lists show it (`api/models.rs` `IssueSummary`). */
export const IssueSummary = z.object({
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
  is_muted: z.boolean()
});
export type IssueSummary = z.infer<typeof IssueSummary>;

/** `GET /issues/{id}`: the summary plus what `issues/base.html` shows (`issues/pages.rs` `IssueDetail`). */
export const Issue = IssueSummary.extend({
  friendly_id: z.string(),
  title: z.string(),
  last_frame_filename: z.string(),
  last_frame_module: z.string(),
  last_frame_function: z.string(),
  is_resolved_unconditionally: z.boolean(),
  is_resolved_by_next_release: z.boolean(),
  fixed_at: z.array(z.string()),
  events_at: z.array(z.string()),
  unmute_after: z.string().nullable(),
  /** `[{"period", "nr_of_periods", "volume"}]`, as stored. */
  unmute_on_volume_based_conditions: z.unknown(),
  project_has_releases: z.boolean(),
  latest_release: z.string().nullable(),
  /** The project's banners (its ingest quota). */
  project_warnings: z.array(z.string())
});
export type Issue = z.infer<typeof Issue>;

export const VolumeBasedCondition = z.object({
  period: z.string(),
  nr_of_periods: z.number(),
  volume: z.number()
});
export type VolumeBasedCondition = z.infer<typeof VolumeBasedCondition>;

export const TagCount = z.object({
  /** null for the "other" remainder. */
  value: z.string().nullable(),
  count: z.number(),
  pct: z.number()
});
export type TagCount = z.infer<typeof TagCount>;

export const KeyTags = z.object({
  key: z.string(),
  values: z.array(TagCount),
  other: TagCount.nullable()
});
export type KeyTags = z.infer<typeof KeyTags>;

export const Grouping = z.object({
  grouping_mechanism: z.string(),
  grouping_key: z.string()
});
export type Grouping = z.infer<typeof Grouping>;

export const TurningPoint = z.object({
  id: z.number(),
  kind: z.number(),
  timestamp: z.string(),
  /** Upstream's parsed metadata, e.g. `{"mute_for": {...}}`; null when unreadable. */
  metadata: z.unknown(),
  comment: z.string(),
  user: z.object({ id: z.number(), display_name: z.string(), gravatar_sha256: z.string() }).nullable(),
  triggering_event: z.object({ id: z.string(), event_id: z.string(), digest_order: z.number() }).nullable()
});
export type TurningPoint = z.infer<typeof TurningPoint>;

export const HourCount = z.object({
  bucket: z.string(),
  count: z.number(),
  digest_order: z.number().nullable()
});
export type HourCount = z.infer<typeof HourCount>;

/** `GET /issues/{id}/counts`: hourly counts for the sparkline. */
export const Counts = z.object({
  since: z.string(),
  until: z.string(),
  hours: z.array(HourCount),
  matching: z.array(HourCount).nullable()
});
export type Counts = z.infer<typeof Counts>;

export const EventSummary = z.object({
  id: z.string(),
  event_id: z.string(),
  issue_id: z.string(),
  digest_order: z.number(),
  ingested_at: z.string(),
  timestamp: z.string(),
  level: z.string(),
  release: z.string(),
  environment: z.string(),
  calculated_type: z.string(),
  calculated_value: z.string()
});
export type EventSummary = z.infer<typeof EventSummary>;

export const Event = EventSummary.extend({
  platform: z.string(),
  server_name: z.string(),
  sdk_name: z.string(),
  sdk_version: z.string(),
  /** The payload as the SDK sent it (arbitrary JSON); null when stored in upstream's file storage. */
  data: z.unknown()
});
export type Event = z.infer<typeof Event>;

export const Nav = z.object({
  first: z.number().nullable(),
  prev: z.number().nullable(),
  next: z.number().nullable(),
  last: z.number().nullable(),
  count: z.number()
});
export type Nav = z.infer<typeof Nav>;

/** Label/value pairs, serialized as 2-element arrays. */
const InfoList = z.array(z.tuple([z.string(), z.unknown()]));
export type InfoList = z.infer<typeof InfoList>;

/** `event_display.rs` `EventDisplay`. Entries and frames are the SDK's JSON (plus a few markers); read permissively. */
export const EventDisplay = z.object({
  title: z.string(),
  request_repr: z.string(),
  stacktrace: z.object({
    entries: z.array(z.unknown()),
    thread_stacktraces: z.array(z.unknown()),
    stack_of_plates: z.boolean(),
    multiple_thread_stacktraces: z.boolean()
  }),
  key_info: InfoList,
  logentry_info: InfoList,
  deployment_info: InfoList,
  contexts: z.unknown(),
  breadcrumbs: z.array(z.unknown()).nullable(),
  /** "Sourcemap IDs": (file name, "<debug id>  (uploaded)" or "<debug id>  (not uploaded)"). */
  sourcemaps_images: z.array(z.tuple([z.string(), z.string()]))
});
export type EventDisplay = z.infer<typeof EventDisplay>;

export const EventPage = z.object({
  event: Event,
  nav: Nav,
  display: EventDisplay.nullable(),
  /** The tags stored for the event, by key. */
  tags: z.array(z.tuple([z.string(), z.string()]))
});
export type EventPage = z.infer<typeof EventPage>;

const PagedEvents = z.object({ items: z.array(EventSummary), offset: z.number(), limit: z.number() });

const ActionResult = z.object({ applied: z.boolean(), issue: IssueSummary.nullable() });

const q = { name: "q", type: "Query", schema: z.string().optional() } as const;

const endpoints = makeApi([
  {
    method: "get",
    path: "/issues/:issueId",
    alias: "issue",
    response: Issue
  },
  {
    method: "post",
    path: "/issues/:issueId/actions",
    alias: "action",
    parameters: [{ name: "body", type: "Body", schema: z.object({ action: z.string() }) }],
    response: ActionResult
  },
  {
    method: "get",
    path: "/issues/:issueId/tags",
    alias: "tags",
    parameters: [{ name: "all", type: "Query", schema: z.boolean().optional() }],
    response: z.array(KeyTags)
  },
  {
    method: "get",
    path: "/issues/:issueId/groupings",
    alias: "groupings",
    response: z.array(Grouping)
  },
  {
    method: "get",
    path: "/issues/:issueId/history",
    alias: "history",
    response: z.array(TurningPoint)
  },
  {
    method: "post",
    path: "/issues/:issueId/comments",
    alias: "addComment",
    parameters: [{ name: "body", type: "Body", schema: z.object({ comment: z.string() }) }],
    response: z.object({ id: z.number().nullable() })
  },
  {
    method: "patch",
    path: "/issues/:issueId/comments/:commentId",
    alias: "editComment",
    parameters: [{ name: "body", type: "Body", schema: z.object({ comment: z.string() }) }],
    response: z.unknown()
  },
  {
    method: "delete",
    path: "/issues/:issueId/comments/:commentId",
    alias: "deleteComment",
    response: z.unknown()
  },
  {
    method: "get",
    path: "/issues/:issueId/counts",
    alias: "counts",
    parameters: [q],
    response: Counts
  },
  {
    method: "get",
    path: "/issues/:issueId/events",
    alias: "events",
    parameters: [
      q,
      { name: "offset", type: "Query", schema: z.number().optional() },
      { name: "limit", type: "Query", schema: z.number().optional() }
    ],
    response: PagedEvents
  },
  {
    method: "get",
    path: "/issues/:issueId/events/:selector",
    alias: "event",
    parameters: [q],
    response: EventPage
  },
  {
    method: "get",
    path: "/issues/:issueId/events/:digestOrder/:nav",
    alias: "eventNav",
    parameters: [q],
    response: EventPage
  },
  {
    method: "get",
    path: "/events/:eventId",
    alias: "eventById",
    response: Event
  }
]);

export const issueApi = new Zodios("/api/v1", endpoints, { axiosInstance: http });
