import { useParams } from "react-router";

import Load from "common/components/Load";
import { routes } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { useTitle } from "common/hooks/useTitle";
import { useSetNavProject } from "common/layouts/NavProject";
import { useFlash } from "modules/teams/components/Flash";
import MemberInvite from "modules/teams/components/MemberInvite";
import { projectsApi } from "services/projects";

/** Upstream's `project_members_invite.html` (project admins only). */
export default function ProjectMembersInvite() {
  const { projectId = "" } = useParams();
  const initial = useFlash();
  const loaded = useLoad(() => projectsApi.projectSettings({ params: { projectId } }), [projectId]);
  useSetNavProject(loaded.data);
  useTitle(loaded.data ? ["Invite members", loaded.data.name] : null);
  return (
    <Load loaded={loaded}>
      {(project) => (
        <MemberInvite
          key={project.id}
          kind="projects"
          id={String(project.id)}
          name={project.name}
          noun="project"
          headingClass="text-4xl my-4 font-bold"
          membersUrl={routes.projectMembers(project.id)}
          initial={initial}
        />
      )}
    </Load>
  );
}
