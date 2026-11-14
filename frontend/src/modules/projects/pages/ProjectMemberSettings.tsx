import { useParams } from "react-router";

import { routes } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { useTitle } from "common/hooks/useTitle";
import { useSetNavProject } from "common/layouts/NavProject";
import { useUser } from "contexts/session";
import { useFlash, type Message } from "modules/teams/components/Flash";
import MemberSettingsPage, { type MemberSettingsData } from "modules/teams/components/MemberSettingsPage";
import { membersApi } from "services/members";
import { myProject, projectsApi } from "services/projects";
import type { Me } from "services/session";
import { myTeam, ROLE_ADMIN } from "services/teams";

function yesNo(value: boolean): string {
  return value ? "Yes" : "No";
}

/** `MyProjectMembershipForm`'s "unknown" choice: your team membership's setting if it has one, else your own. */
async function emailAlertsDefault(teamId: string | null, me: Me): Promise<string> {
  const team = teamId ? await myTeam(teamId) : null;
  if (team && team.my_send_email_alerts !== null) {
    return `Default (${yesNo(team.my_send_email_alerts)}, as per team membership settings)`;
  }
  return `Default (${yesNo(me.send_email_alerts)}, as per user settings)`;
}

/**
 * Upstream's `project_member_settings`: you need an accepted membership (else on to the invitation); someone else's
 * membership only as a project admin.
 */
async function load(projectId: number, userId: number, me: Me, messages: Message[]): Promise<MemberSettingsData> {
  const mine = await myProject(projectId);
  if (!mine) {
    return { go: "forbidden", message: "You are not a member of this project" };
  }
  if (!mine.my_accepted) {
    return { go: "accept", to: routes.projectMembersAccept(projectId) };
  }
  const common = { kind: "projects" as const, id: String(projectId), noun: "project", userId, messages };
  if (userId === me.id) {
    return {
      go: "form",
      form: {
        ...common,
        name: mine.name,
        username: me.username,
        thisIsYou: true,
        editRole: mine.my_role === ROLE_ADMIN || me.is_superuser,
        role: mine.my_role ?? 0,
        sendEmailAlerts: mine.my_send_email_alerts,
        emailAlertsDefault: await emailAlertsDefault(mine.team_id, me),
        doneUrl: routes.projects()
      }
    };
  }
  // project admins only (the API answers 403 otherwise)
  const [project, list] = await Promise.all([
    projectsApi.projectSettings({ params: { projectId: String(projectId) } }),
    membersApi.members({ params: { kind: "projects", id: String(projectId) } })
  ]);
  const member = list.members.find((m) => m.user_id === userId);
  if (!member) {
    return { go: "notFound" };
  }
  return {
    go: "form",
    form: {
      ...common,
      name: project.name,
      username: member.username,
      thisIsYou: false,
      editRole: true,
      role: member.role,
      sendEmailAlerts: null,
      emailAlertsDefault: "",
      doneUrl: routes.projectMembers(projectId)
    }
  };
}

/** Upstream's `project_member_settings.html`. */
export default function ProjectMemberSettings() {
  const { projectId = "", userId = "" } = useParams();
  const me = useUser();
  const { messages } = useFlash();
  const loaded = useLoad(() => load(Number(projectId), Number(userId), me, messages), [projectId, userId]);
  const form = loaded.data?.go === "form" ? loaded.data.form : null;
  useSetNavProject(form && { id: Number(form.id), name: form.name });
  useTitle(form ? ["Membership settings", form.name] : null);
  return <MemberSettingsPage loaded={loaded} />;
}
