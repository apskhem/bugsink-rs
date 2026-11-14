import { useParams } from "react-router";

import { routes } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { useTitle } from "common/hooks/useTitle";
import { useUser } from "contexts/session";
import { useFlash, type Message } from "modules/teams/components/Flash";
import MemberSettingsPage, { type MemberSettingsData } from "modules/teams/components/MemberSettingsPage";
import { membersApi } from "services/members";
import type { Me } from "services/session";
import { myTeam, ROLE_ADMIN, teamsApi } from "services/teams";

/**
 * Upstream's `team_member_settings`: you need an accepted membership (else on to the invitation); someone else's
 * membership only as a superuser who is also a team admin.
 */
async function load(teamId: string, userId: number, me: Me, messages: Message[]): Promise<MemberSettingsData> {
  const mine = await myTeam(teamId);
  if (!mine) {
    return { go: "forbidden", message: "You are not a member of this team" };
  }
  if (!mine.my_accepted) {
    return { go: "accept", to: routes.teamMembersAccept(teamId) };
  }
  const common = { kind: "teams" as const, id: teamId, noun: "team", userId, messages };
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
        emailAlertsDefault: `User-default (${me.send_email_alerts ? "Yes" : "No"})`,
        doneUrl: routes.teams()
      }
    };
  }
  if (!me.is_superuser || mine.my_role !== ROLE_ADMIN) {
    return { go: "forbidden", message: "You are not an admin of this team" };
  }
  const [team, list] = await Promise.all([
    teamsApi.team({ params: { teamId } }),
    membersApi.members({ params: { kind: "teams", id: teamId } })
  ]);
  const member = list.members.find((m) => m.user_id === userId);
  if (!member) {
    return { go: "notFound" };
  }
  return {
    go: "form",
    form: {
      ...common,
      name: team.name,
      username: member.username,
      thisIsYou: false,
      editRole: true,
      role: member.role,
      sendEmailAlerts: null,
      emailAlertsDefault: "",
      doneUrl: routes.teamMembers(teamId)
    }
  };
}

/** Upstream's `team_member_settings.html`. */
export default function TeamMemberSettings() {
  const { teamId = "", userId = "" } = useParams();
  const me = useUser();
  const { messages } = useFlash();
  const loaded = useLoad(() => load(teamId, Number(userId), me, messages), [teamId, userId]);
  useTitle(loaded.data?.go === "form" ? ["Membership settings", loaded.data.form.name] : null);
  return <MemberSettingsPage loaded={loaded} />;
}
