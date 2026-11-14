import { Link, useParams } from "react-router";

import Load from "common/components/Load";
import { routes } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { useTitle } from "common/hooks/useTitle";
import { useSetNavProject } from "common/layouts/NavProject";
import { useFlash } from "modules/teams/components/Flash";
import Members from "modules/teams/components/Members";
import { membersApi } from "services/members";
import { myProject, projectsApi } from "services/projects";
import { teamsApi } from "services/teams";

/**
 * The project's team's name, for the "<team> Members" link: the project settings don't have it, so from the project
 * list (as a project member) or else the team itself (superusers and team admins, the others who get here).
 */
async function teamName(projectId: number, teamId: string): Promise<string | null> {
  try {
    const project = await myProject(projectId);
    if (project?.team_name) {
      return project.team_name;
    }
    return (await teamsApi.team({ params: { teamId } })).name;
  } catch {
    return null;
  }
}

async function load(projectId: string) {
  const [project, list] = await Promise.all([
    projectsApi.projectSettings({ params: { projectId } }),
    membersApi.members({ params: { kind: "projects", id: projectId } })
  ]);
  const team = project.team_id ? await teamName(project.id, project.team_id) : null;
  return { project, list, team };
}

/** Upstream's `project_members.html` (project admins only). */
export default function ProjectMembers() {
  const { projectId = "" } = useParams();
  const initial = useFlash();
  const loaded = useLoad(() => load(projectId), [projectId]);
  useSetNavProject(loaded.data?.project);
  useTitle(loaded.data ? ["Members", loaded.data.project.name] : null);
  return (
    <Load loaded={loaded}>
      {({ project, list, team }) => (
        <Members
          key={project.id}
          kind="projects"
          id={String(project.id)}
          name={project.name}
          list={list}
          initial={initial}
          inviteUrl={routes.projectMembersInvite(project.id)}
          memberSettingsUrl={(userId) => routes.projectMemberSettings(project.id, userId)}
          memberLinkClass="text-xl text-cyan-500 font-bold"
          footer={
            <>
              {project.team_id && (
                <>
                  <Link to={routes.teamMembers(project.team_id)} className="text-cyan-500 dark:text-cyan-300 font-bold">
                    {team === null ? " Team Members" : ` ${team} Members`}
                  </Link>{" "}
                  <span className="font-bold text-slate-500 dark:text-slate-300">|</span>{" "}
                </>
              )}
              <Link to={routes.projects()} className="text-cyan-500 dark:text-cyan-300 font-bold">
                Back to Projects
              </Link>
            </>
          }
        />
      )}
    </Load>
  );
}
