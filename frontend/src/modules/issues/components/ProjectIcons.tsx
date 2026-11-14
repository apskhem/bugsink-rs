import type { MouseEvent, ReactNode } from "react";
import { Link, useNavigate } from "react-router";

import { routes } from "common/constants/routes";

import { MegaphoneIcon, MembersIcon, SdkSetupIcon, SettingsIcon } from "./icons";

/** A round icon button; clicking anywhere in the circle follows its link (upstream's `followContainedLink`). */
function IconLink({ to, title, children }: { to: string; title: string; children: ReactNode }) {
  const navigate = useNavigate();
  function follow(event: MouseEvent<HTMLDivElement>) {
    if (event.target instanceof Element && event.target.closest("a")) {
      return; // the link itself handles it
    }
    void navigate(to);
  }
  return (
    <div
      className="rounded-full hover:bg-slate-100 dark:hover:bg-slate-700 p-2 cursor-pointer text-slate-700"
      onClick={follow}
      title={title}
    >
      <Link to={to}>{children}</Link>
    </div>
  );
}

/** The project-related icons in the lower right of a project's issue list. */
export default function ProjectIcons({
  projectId,
  userId,
  isAdmin
}: {
  projectId: number;
  userId: number;
  /** A project admin or a superuser. */
  isAdmin: boolean;
}) {
  return (
    <div className="flex ml-auto justify-end">
      {isAdmin && (
        <>
          <IconLink to={routes.projectMembers(projectId)} title="Project members">
            <MembersIcon />
          </IconLink>
          <IconLink to={routes.projectEdit(projectId)} title="Project settings">
            <SettingsIcon />
          </IconLink>
        </>
      )}
      <IconLink to={routes.projectMemberSettings(projectId, userId)} title="Project membership (notification settings)">
        <MegaphoneIcon />
      </IconLink>
      <IconLink to={routes.projectSdkSetup(projectId)} title="SDK Setup (connect app)">
        <SdkSetupIcon />
      </IconLink>
    </div>
  );
}
