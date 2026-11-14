import { useState } from "react";
import { Link, Navigate, useLocation, useNavigate } from "react-router";

import { LoadError } from "common/components/ErrorPage";
import Load from "common/components/Load";
import { routes } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { useTitle } from "common/hooks/useTitle";
import { useUser } from "contexts/session";
import { LeaveTeamModal } from "modules/teams/components/ConfirmModal";
import { flash, Messages, success, useFlash, warning } from "modules/teams/components/Flash";
import { CircleLink, MembersIcon, SettingsIcon } from "modules/teams/components/ListIcons";
import { primaryLink, secondaryButton } from "modules/teams/components/buttons";
import { useFailure } from "modules/teams/hooks/useFailure";
import { apiError } from "services/http";
import { ROLE_ADMIN, teamsApi, type TeamList as TeamListData, type TeamListItem, type TeamTab } from "services/teams";

const tabClass = (active: boolean) =>
  `p-4 font-bold text-xl hover:bg-slate-200 dark:hover:bg-slate-800 ${
    active
      ? "text-cyan-500 dark:text-cyan-400 border-cyan-500 border-b-4 "
      : "text-slate-500 dark:text-slate-300 hover:border-b-4 hover:border-slate-400 dark:hover:border-slate-500"
  }`;

const rowButton = `${secondaryButton} ml-1 whitespace-nowrap`;

/**
 * Upstream's `team_list.html`: "My Teams" (incl. invitations) and "Other Teams", with joining and leaving (after
 * confirming). Without a tab, upstream redirects to the most informative one.
 */
export default function TeamList({ tab }: { tab?: TeamTab }) {
  useTitle(["Teams"]);
  const location = useLocation();
  // every navigation (also to this same page, after an action) loads the list again
  const loaded = useLoad(() => teamsApi.teamList({ queries: tab ? { tab } : {} }), [tab, location.key]);
  return (
    <Load loaded={loaded}>
      {(data) =>
        tab ? <Teams tab={tab} data={data} /> : <Navigate to={routes.teams(data.tab)} replace state={location.state} />
      }
    </Load>
  );
}

function Teams({ tab, data }: { tab: TeamTab; data: TeamListData }) {
  const user = useUser();
  const navigate = useNavigate();
  const { messages } = useFlash();
  const { failure, fail } = useFailure();
  const [leaving, setLeaving] = useState<TeamListItem | null>(null);

  async function join(team: TeamListItem) {
    try {
      await teamsApi.joinTeam(undefined, { params: { teamId: team.id } });
    } catch (error) {
      fail(error);
      return;
    }
    await navigate(routes.teamMemberSettings(team.id, user.id), {
      state: flash([success(`You have joined the team "${team.name}"`)])
    });
  }

  async function leave(team: TeamListItem) {
    setLeaving(null);
    let message;
    try {
      await teamsApi.leaveTeam(undefined, { params: { teamId: team.id } });
      message = success(`You have left the team "${team.name}"`);
    } catch (error) {
      const failed = apiError(error);
      if (failed?.status !== 409) {
        fail(error);
        return;
      }
      // the last admin
      message = warning(failed.detail);
    }
    await navigate(routes.teams("mine"), { state: flash([message]) });
  }

  if (failure) {
    return <LoadError error={failure.error} />;
  }

  return (
    <>
      {leaving && <LeaveTeamModal onCancel={() => setLeaving(null)} onConfirm={() => void leave(leaving)} />}

      <div className="m-4 flex flex-row items-end">
        <div>
          <h1 className="text-4xl mt-4 font-bold">Teams</h1>
        </div>

        <div className="ml-auto">
          {data.can_create && (
            <div>
              <Link className={`block ${primaryLink}`} to={routes.teamNew()}>
                New Team
              </Link>
            </div>
          )}
        </div>
      </div>

      <div className="m-4">
        <Messages messages={messages} className="mb-4" />

        <div className="flex bg-slate-50 dark:bg-slate-800 mt-4 items-end">
          <div className="flex">
            <Link to={routes.teams("mine")}>
              <div className={tabClass(tab === "mine")}>My Teams</div>
            </Link>
            <Link to={routes.teams("other")}>
              <div className={tabClass(tab === "other")}>Other Teams</div>
            </Link>
          </div>
        </div>

        <div>
          <table className="w-full">
            <tbody>
              {data.items.map((team) => {
                const member = team.my_role !== null;
                const isAdmin = team.my_role === ROLE_ADMIN;
                return (
                  <tr
                    key={team.id}
                    className="bg-white dark:bg-slate-900 border-slate-200 dark:border-slate-700 border-b-2"
                  >
                    <td className="w-full p-4">
                      <div className="text-xl font-bold text-slate-800 dark:text-slate-100">{team.name}</div>
                      <div>
                        {team.project_count} projects | {team.member_count} members
                        {member && (
                          <>
                            {" | "}
                            <Link
                              to={routes.teamMemberSettings(team.id, user.id)}
                              className="font-bold text-cyan-500 dark:text-cyan-400"
                            >
                              my settings
                            </Link>
                          </>
                        )}
                      </div>
                    </td>

                    <td className="pr-2 text-center">
                      {member &&
                        (!team.my_accepted ? (
                          <span className="bg-slate-100 dark:bg-slate-700 rounded-2xl px-4 py-2 ml-2 text-sm">
                            You're{" "}invited!
                          </span>
                        ) : (
                          isAdmin && (
                            <span className="bg-cyan-100 dark:bg-cyan-900 rounded-2xl px-4 py-2 ml-2 text-sm whitespace-nowrap">
                              Admin
                            </span>
                          )
                        ))}
                    </td>

                    <td className="pr-2">
                      {(isAdmin || user.is_superuser) && (
                        <CircleLink to={routes.teamMembers(team.id)} title="Team members">
                          <MembersIcon />
                        </CircleLink>
                      )}
                    </td>

                    <td className="pr-2">
                      {(isAdmin || user.is_superuser) && (
                        <CircleLink to={routes.teamEdit(team.id)} title="Team settings">
                          <SettingsIcon />
                        </CircleLink>
                      )}
                    </td>

                    <td className="pr-2">
                      {member ? (
                        !team.my_accepted ? (
                          <div>
                            <Link
                              to={routes.teamMembersAccept(team.id)}
                              className="font-bold text-cyan-500 dark:text-cyan-400 whitespace-nowrap"
                            >
                              Invitation
                            </Link>
                          </div>
                        ) : (
                          <div>
                            <button type="button" className={rowButton} onClick={() => setLeaving(team)}>
                              Leave
                            </button>
                          </div>
                        )
                      ) : (
                        team.joinable && (
                          <div>
                            <button type="button" className={rowButton} onClick={() => void join(team)}>
                              Join
                            </button>
                          </div>
                        )
                      )}
                    </td>
                  </tr>
                );
              })}
              {!data.items.length && (
                <tr className="bg-white dark:bg-slate-900 border-slate-200 dark:border-slate-700 border-b-2">
                  <td className="w-full p-4">No teams found.</td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>
    </>
  );
}
