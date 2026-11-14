import { Link, Navigate, useLocation, useNavigate } from "react-router";

import CompactSparkline from "common/components/CompactSparkline";
import { LoadError } from "common/components/ErrorPage";
import Load from "common/components/Load";
import { routes } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { useTitle } from "common/hooks/useTitle";
import { useUser } from "contexts/session";
import { AlertsIcon, SdkIcon } from "modules/projects/components/ProjectIcons";
import { flash, Messages, success, useFlash } from "modules/teams/components/Flash";
import { CircleLink, MembersIcon, SettingsIcon } from "modules/teams/components/ListIcons";
import { primaryLink, secondaryButton } from "modules/teams/components/buttons";
import { useFailure } from "modules/teams/hooks/useFailure";
import {
  projectsApi,
  type ProjectList as ProjectListData,
  type ProjectListItem,
  type ProjectTab
} from "services/projects";
import { ROLE_ADMIN } from "services/teams";

const tabClass = (active: boolean) =>
  `p-4 font-bold text-xl hover:bg-slate-200 dark:hover:bg-slate-800 ${
    active
      ? "text-cyan-500 dark:text-cyan-300 border-cyan-500 border-b-4 "
      : "text-slate-500 dark:text-slate-300 hover:border-b-4 hover:border-slate-400"
  }`;

/**
 * Upstream's `project_list.html`: "My Projects" (incl. invitations), "Team Projects" (your teams' other projects) and
 * "Other Projects", each with its last 24 hours of events, and joining and leaving. Without a tab, upstream redirects
 * to the first one that isn't empty.
 */
export default function ProjectList({ tab }: { tab?: ProjectTab }) {
  useTitle(["Projects"]);
  const location = useLocation();
  // every navigation (also to this same page, after an action) loads the list again
  const loaded = useLoad(() => projectsApi.projectList({ queries: tab ? { tab } : {} }), [tab, location.key]);
  return (
    <Load loaded={loaded}>
      {(data) =>
        tab ? (
          <Projects tab={tab} data={data} />
        ) : (
          <Navigate to={routes.projects(data.tab)} replace state={location.state} />
        )
      }
    </Load>
  );
}

function Projects({ tab, data }: { tab: ProjectTab; data: ProjectListData }) {
  const user = useUser();
  const navigate = useNavigate();
  const { messages } = useFlash();
  const { failure, fail } = useFailure();

  async function join(project: ProjectListItem) {
    try {
      await projectsApi.joinProject(undefined, { params: { projectId: String(project.id) } });
    } catch (error) {
      fail(error);
      return;
    }
    await navigate(routes.projectMemberSettings(project.id, user.id), {
      state: flash([success(`You have joined the project "${project.name}"`)])
    });
  }

  async function leave(project: ProjectListItem) {
    try {
      await projectsApi.leaveProject(undefined, { params: { projectId: String(project.id) } });
    } catch (error) {
      fail(error);
      return;
    }
    // upstream shows the same tab again
    await navigate(routes.projects(tab));
  }

  if (failure) {
    return <LoadError error={failure.error} />;
  }

  return (
    <>
      <div className="m-4 flex flex-row items-end">
        <div>
          <h1 className="text-4xl mt-4 font-bold">Projects</h1>
        </div>

        <div className="ml-auto">
          {data.can_create && (
            <div>
              <Link className={`block ${primaryLink}`} to={routes.projectNew()}>
                New Project
              </Link>
            </div>
          )}
        </div>
      </div>

      <div className="m-4">
        <Messages messages={messages} className="mb-4" />

        <div className="flex bg-slate-50 dark:bg-slate-800 mt-4 items-end">
          <div className="flex">
            <Link to={routes.projects("mine")}>
              <div className={tabClass(tab === "mine")}>My Projects</div>
            </Link>
            <Link to={routes.projects("teams")}>
              <div className={tabClass(tab === "teams")}>Team Projects</div>
            </Link>
            <Link to={routes.projects("other")}>
              <div className={tabClass(tab === "other")}>Other Projects</div>
            </Link>
          </div>
        </div>

        <div>
          <table className="w-full">
            <tbody>
              {data.items.map((project) => {
                const member = project.my_role !== null;
                const isAdmin = project.my_role === ROLE_ADMIN;
                return (
                  <tr
                    key={project.id}
                    className="bg-white dark:bg-slate-900 border-slate-200 dark:border-slate-700 border-b-2"
                  >
                    <td className="w-full p-4 pr-12">
                      <div className="flex items-end gap-4">
                        <div className="min-w-0 flex-1">
                          <div>
                            {member || user.is_superuser ? (
                              <Link
                                to={routes.issues(project.id)}
                                className="text-xl text-cyan-500 dark:text-cyan-300 font-bold"
                              >
                                {project.name}
                              </Link>
                            ) : (
                              <span className="text-xl text-slate-800 dark:text-slate-100 font-bold">
                                {project.name}
                              </span>
                            )}
                          </div>
                          <div>
                            {project.team_name} | {project.member_count} members
                            {project.open_issue_count !== null
                              ? ` | ${project.open_issue_count} open issues`
                              : " | many issues"}
                            {member && (
                              <>
                                {" | "}
                                <Link
                                  to={routes.projectMemberSettings(project.id, user.id)}
                                  className="font-bold text-cyan-500 dark:text-cyan-300"
                                >
                                  my settings
                                </Link>
                              </>
                            )}
                          </div>
                        </div>
                        <div className="flex-none">
                          <CompactSparkline counts={project.sparkline} />
                        </div>
                      </div>
                    </td>

                    <td className="pr-2 text-center">
                      {member &&
                        (!project.my_accepted ? (
                          <span className="bg-slate-100 dark:bg-slate-700 rounded-2xl px-4 py-2 ml-2 text-sm whitespace-nowrap">
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
                        <CircleLink to={routes.projectMembers(project.id)} title="Project members">
                          <MembersIcon />
                        </CircleLink>
                      )}
                    </td>

                    <td className="pr-2">
                      {(isAdmin || user.is_superuser) && (
                        <CircleLink to={routes.projectEdit(project.id)} title="Project settings">
                          <SettingsIcon />
                        </CircleLink>
                      )}
                    </td>

                    <td className="pr-2">
                      {(member || user.is_superuser) && (
                        <CircleLink to={routes.projectAlerts(project.id)} title="Alerting Settings">
                          <AlertsIcon />
                        </CircleLink>
                      )}
                    </td>

                    <td className="pr-2">
                      {(member || user.is_superuser) && (
                        <CircleLink to={routes.projectSdkSetup(project.id)} title="SDK setup (connect app)">
                          <SdkIcon />
                        </CircleLink>
                      )}
                    </td>

                    <td className="pr-2">
                      {member ? (
                        !project.my_accepted ? (
                          <div>
                            <Link
                              to={routes.projectMembersAccept(project.id)}
                              className="font-bold text-cyan-500 dark:text-cyan-300 whitespace-nowrap"
                            >
                              Invitation
                            </Link>
                          </div>
                        ) : (
                          <div>
                            <button
                              type="button"
                              className={`${secondaryButton} ml-1 whitespace-nowrap`}
                              onClick={() => void leave(project)}
                            >
                              Leave
                            </button>
                          </div>
                        )
                      ) : (
                        (tab === "teams" || project.joinable || user.is_superuser) && (
                          <div>
                            <button
                              type="button"
                              className="font-bold text-slate-500 dark:text-slate-300 border-slate-300 dark:border-slate-600 pl-4 pr-4 pb-2 pt-2 ml-1 border-2  dark:hover:bg-slate-700 active:ring-3 rounded-md whitespace-nowrap"
                              onClick={() => void join(project)}
                            >
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
                  <td className="w-full p-4">No projects found.</td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>
    </>
  );
}
