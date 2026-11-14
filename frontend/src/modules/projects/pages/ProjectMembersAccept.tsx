import { Navigate, useParams } from "react-router";

import { NotFound } from "common/components/ErrorPage";
import Load from "common/components/Load";
import { routes } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { useTitle } from "common/hooks/useTitle";
import { useSetNavProject } from "common/layouts/NavProject";
import { useUser } from "contexts/session";
import AcceptInvitation from "modules/teams/components/AcceptInvitation";
import { myProject } from "services/projects";

/**
 * Upstream's `project_members_accept.html`: your invitation to the project (an accepted one: on to your settings, as
 * if you had just accepted it).
 */
export default function ProjectMembersAccept() {
  const { projectId = "" } = useParams();
  const user = useUser();
  const loaded = useLoad(() => myProject(Number(projectId)), [projectId]);
  useSetNavProject(loaded.data);
  useTitle(loaded.data ? ["Invitation", loaded.data.name] : null);
  return (
    <Load loaded={loaded}>
      {(project) =>
        !project || project.my_role === null ? (
          <NotFound />
        ) : project.my_accepted ? (
          <Navigate to={routes.projectMemberSettings(project.id, user.id)} replace />
        ) : (
          <AcceptInvitation
            kind="projects"
            id={String(project.id)}
            name={project.name}
            noun="project"
            role={project.my_role}
            headingClass="text-4xl my-4 font-bold"
            acceptedUrl={routes.projectMemberSettings(project.id, user.id)}
          />
        )
      }
    </Load>
  );
}
