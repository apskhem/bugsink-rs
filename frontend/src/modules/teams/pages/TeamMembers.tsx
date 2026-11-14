import { Link, useParams } from "react-router";

import Load from "common/components/Load";
import { routes } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { useTitle } from "common/hooks/useTitle";
import { useFlash } from "modules/teams/components/Flash";
import Members from "modules/teams/components/Members";
import { membersApi } from "services/members";
import { teamsApi } from "services/teams";

/** Upstream's `team_members.html` (team admins only). */
export default function TeamMembers() {
  const { teamId = "" } = useParams();
  const initial = useFlash();
  const loaded = useLoad(
    () =>
      Promise.all([
        teamsApi.team({ params: { teamId } }),
        membersApi.members({ params: { kind: "teams", id: teamId } })
      ]),
    [teamId]
  );
  useTitle(loaded.data ? ["Members", loaded.data[0].name] : null);
  return (
    <Load loaded={loaded}>
      {([team, list]) => (
        <Members
          key={team.id}
          kind="teams"
          id={team.id}
          name={team.name}
          list={list}
          initial={initial}
          inviteUrl={routes.teamMembersInvite(team.id)}
          memberSettingsUrl={(userId) => routes.teamMemberSettings(team.id, userId)}
          memberLinkClass="text-xl text-cyan-500 dark:text-cyan-300 font-bold"
          footer={
            <Link to={routes.teams()} className="text-cyan-500 dark:text-cyan-300 font-bold">
              Back to Teams
            </Link>
          }
        />
      )}
    </Load>
  );
}
