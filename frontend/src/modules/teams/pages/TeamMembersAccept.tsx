import { Navigate, useParams } from "react-router";

import { NotFound } from "common/components/ErrorPage";
import Load from "common/components/Load";
import { routes } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { useTitle } from "common/hooks/useTitle";
import { useUser } from "contexts/session";
import AcceptInvitation from "modules/teams/components/AcceptInvitation";
import { myTeam } from "services/teams";

/**
 * Upstream's `team_members_accept.html`: your invitation to the team (an accepted one: on to your settings, as if you
 * had just accepted it).
 */
export default function TeamMembersAccept() {
  const { teamId = "" } = useParams();
  const user = useUser();
  const loaded = useLoad(() => myTeam(teamId), [teamId]);
  useTitle(loaded.data ? ["Invitation", loaded.data.name] : null);
  return (
    <Load loaded={loaded}>
      {(team) =>
        !team || team.my_role === null ? (
          <NotFound />
        ) : team.my_accepted ? (
          <Navigate to={routes.teamMemberSettings(team.id, user.id)} replace />
        ) : (
          <AcceptInvitation
            kind="teams"
            id={team.id}
            name={team.name}
            noun="team"
            role={team.my_role}
            headingClass="text-4xl mt-4 font-bold"
            acceptedUrl={routes.teamMemberSettings(team.id, user.id)}
          />
        )
      }
    </Load>
  );
}
