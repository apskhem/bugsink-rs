import { useParams } from "react-router";

import Load from "common/components/Load";
import { routes } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { useTitle } from "common/hooks/useTitle";
import { useFlash } from "modules/teams/components/Flash";
import MemberInvite from "modules/teams/components/MemberInvite";
import { teamsApi } from "services/teams";

/** Upstream's `team_members_invite.html` (team admins only). */
export default function TeamMembersInvite() {
  const { teamId = "" } = useParams();
  const initial = useFlash();
  const loaded = useLoad(() => teamsApi.team({ params: { teamId } }), [teamId]);
  useTitle(loaded.data ? ["Invite members", loaded.data.name] : null);
  return (
    <Load loaded={loaded}>
      {(team) => (
        <MemberInvite
          key={team.id}
          kind="teams"
          id={team.id}
          name={team.name}
          noun="team"
          headingClass="text-4xl mt-4 font-bold"
          membersUrl={routes.teamMembers(team.id)}
          initial={initial}
        />
      )}
    </Load>
  );
}
