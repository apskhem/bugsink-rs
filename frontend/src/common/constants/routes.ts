/**
 * Upstream's URLs: emails (from either side) link to them, so the frontend routes the same paths.
 */
export const routes = {
  home: () => "/",
  // Django's `redirect_to_login` leaves the slashes
  login: (next?: string) =>
    next ? `/accounts/login/?next=${encodeURIComponent(next).replace(/%2F/g, "/")}` : "/accounts/login/",
  logout: () => "/accounts/logout/",
  signup: () => "/accounts/signup/",
  resendConfirmation: () => "/accounts/resend-confirmation/",
  confirmEmail: (token: string) => `/accounts/confirm-email/${token}/`,
  requestResetPassword: () => "/accounts/request-reset-password/",
  resetPassword: (token: string) => `/accounts/reset-password/${token}/`,
  preferences: () => "/accounts/preferences/",
  changePassword: () => "/accounts/change-password/",

  users: () => "/users/",
  userEdit: (userId: number) => `/users/${userId}/edit/`,
  authTokens: () => "/bsmain/auth_tokens/",
  authTokenCreate: () => "/bsmain/auth_tokens/create/",
  settings: () => "/settings/",
  counts: () => "/counts/",
  openApi: () => "/api/canonical/0/schema/swagger-ui/",

  teams: (tab?: "mine" | "other") => (tab ? `/teams/${tab}/` : "/teams/"),
  teamNew: () => "/teams/new/",
  teamEdit: (teamId: string) => `/teams/${teamId}/edit/`,
  teamMembers: (teamId: string) => `/teams/${teamId}/members/`,
  teamMembersInvite: (teamId: string) => `/teams/${teamId}/members/invite/`,
  teamMembersAccept: (teamId: string) => `/teams/${teamId}/members/accept/`,
  teamMembersAcceptNewUser: (teamId: string, token: string) => `/teams/${teamId}/members/accept/${token}/`,
  teamMemberSettings: (teamId: string, userId: number) => `/teams/${teamId}/members/settings/${userId}/`,

  projects: (tab?: "mine" | "teams" | "other") => (tab ? `/projects/${tab}/` : "/projects/"),
  projectNew: () => "/projects/new/",
  projectEdit: (projectId: number) => `/projects/${projectId}/edit/`,
  projectMembers: (projectId: number) => `/projects/${projectId}/members/`,
  projectMembersInvite: (projectId: number) => `/projects/${projectId}/members/invite/`,
  projectMembersAccept: (projectId: number) => `/projects/${projectId}/members/accept/`,
  projectMembersAcceptNewUser: (projectId: number | string, token: string) =>
    `/projects/${projectId}/members/accept/${token}/`,
  projectMemberSettings: (projectId: number, userId: number) => `/projects/${projectId}/members/settings/${userId}/`,
  projectSdkSetup: (projectId: number, platform?: string) =>
    platform ? `/projects/${projectId}/sdk-setup/${platform}/` : `/projects/${projectId}/sdk-setup/`,
  projectAlerts: (projectId: number) => `/projects/${projectId}/alerts/`,
  projectMessagingServiceAdd: (projectId: number) => `/projects/${projectId}/alerts/service/add/`,
  projectMessagingServiceEdit: (projectId: number, serviceId: number) =>
    `/projects/${projectId}/alerts/service/${serviceId}/edit/`,

  /** The global issue list (all projects you have issue access to). */
  globalIssues: (state: IssueState = "open") => (state === "open" ? "/issues/" : `/issues/${state}/`),
  /** A project's issue list. Upstream's "unresolved" path has no trailing slash. */
  issues: (projectId: number, state: IssueState = "open") =>
    state === "open" ? `/issues/${projectId}/` : `/issues/${projectId}/${state}${state === "unresolved" ? "" : "/"}`,
  /** An issue's event page: `event` is a digest order, an event id, "first" or "last". */
  event: (issueId: string, event: string | number, tab: EventTab = "stacktrace") =>
    `/issues/issue/${issueId}/event/${event}/${tab === "stacktrace" ? "" : `${tab}/`}`,
  issueTags: (issueId: string) => `/issues/issue/${issueId}/tags/`,
  issueHistory: (issueId: string) => `/issues/issue/${issueId}/history/`,
  issueGrouping: (issueId: string) => `/issues/issue/${issueId}/grouping/`,
  issueEvents: (issueId: string) => `/issues/issue/${issueId}/events/`,
  eventById: (eventId: string) => `/issues/event/${eventId}/`
};

export type IssueState = "open" | "unresolved" | "resolved" | "muted" | "all";
export type EventTab = "stacktrace" | "details" | "breadcrumbs";
