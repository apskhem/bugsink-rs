import { makeApi, Zodios } from "@zodios/core";
import { z } from "zod";

import { http } from "./http";
import { projectsApi } from "./projects";

/** The SDK setup page's data; upstream's page polls the count to see the first event arrive. */
export const SdkSetup = z.object({
  dsn: z.string(),
  digested_event_count: z.number()
});
export type SdkSetup = z.infer<typeof SdkSetup>;

/** Just what these pages need of a project (`GET /projects/{id}` is the project's settings, for its admins). */
export const ProjectName = z.object({ id: z.number(), name: z.string() });
export type ProjectName = z.infer<typeof ProjectName>;

/** What can be added: (kind, label) as upstream's `get_alert_service_kind_choices`. */
export const ServiceKind = z.object({ kind: z.string(), label: z.string() });
export type ServiceKind = z.infer<typeof ServiceKind>;

export const MessagingService = z.object({
  id: z.number(),
  display_name: z.string(),
  kind: z.string(),
  /** The kind's config, e.g. `{"webhook_url": ...}`. */
  config: z.unknown(),
  last_failure_timestamp: z.string().nullable(),
  last_failure_status_code: z.number().nullable(),
  last_failure_response_text: z.string().nullable(),
  last_failure_is_json: z.boolean().nullable(),
  last_failure_error_type: z.string().nullable(),
  last_failure_error_message: z.string().nullable()
});
export type MessagingService = z.infer<typeof MessagingService>;

export const MessagingServices = z.object({
  kinds: z.array(ServiceKind),
  services: z.array(MessagingService)
});
export type MessagingServices = z.infer<typeof MessagingServices>;

/** The service's form (`display_name`, `kind` on creation only) plus the kind's config form's fields. */
export const MessagingServiceForm = z.object({
  display_name: z.string(),
  kind: z.string().optional(),
  config: z.record(z.string())
});
export type MessagingServiceForm = z.infer<typeof MessagingServiceForm>;

const endpoints = makeApi([
  { method: "get", path: "/projects/:projectId/sdk-setup", alias: "sdkSetup", response: SdkSetup },
  {
    method: "get",
    path: "/projects/:projectId/messaging-services",
    alias: "messagingServices",
    response: MessagingServices
  },
  {
    method: "post",
    path: "/projects/:projectId/messaging-services",
    alias: "addMessagingService",
    parameters: [{ name: "body", type: "Body", schema: MessagingServiceForm }],
    response: z.object({ id: z.number() })
  },
  {
    method: "patch",
    path: "/projects/:projectId/messaging-services/:serviceId",
    alias: "editMessagingService",
    parameters: [{ name: "body", type: "Body", schema: MessagingServiceForm }],
    response: z.unknown()
  },
  {
    method: "delete",
    path: "/projects/:projectId/messaging-services/:serviceId",
    alias: "removeMessagingService",
    response: z.unknown()
  },
  {
    method: "post",
    path: "/projects/:projectId/messaging-services/:serviceId/test",
    alias: "testMessagingService",
    response: z.unknown()
  }
]);

export const projectSetupApi = new Zodios("/api/v1", endpoints, { axiosInstance: http });

/** The project's name and id, for the navigation (any member may see these pages). */
export async function projectName(projectId: number): Promise<ProjectName | null> {
  return projectsApi.projectSummary({ params: { projectId } }).catch(() => null);
}
