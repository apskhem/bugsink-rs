import { Link, useLocation, useParams, useSearchParams } from "react-router";

import { NotFound } from "common/components/ErrorPage";
import Load from "common/components/Load";
import { routes, type IssueState } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { useTitle } from "common/hooks/useTitle";
import { useSetNavProject } from "common/layouts/NavProject";
import { useUser } from "contexts/session";
import IssueList, { parsePage, parseSort } from "modules/issues/components/IssueList";
import { PAGE_SIZE } from "modules/issues/components/Pagination";
import ProjectIcons from "modules/issues/components/ProjectIcons";
import ProjectWarnings from "modules/issues/components/ProjectWarnings";
import { issuesApi } from "services/issues";
import { projectsApi } from "services/projects";

/** Upstream's `issue_list`: a project's issues in a tab (`state`), sorted, searched (`?q=`) and paged. */
export default function ProjectIssueList({ state }: { state: IssueState }) {
  const { projectId } = useParams();
  const id = Number(projectId);
  return Number.isInteger(id) && id > 0 ? <ProjectIssues projectId={id} state={state} /> : <NotFound />;
}

function ProjectIssues({ projectId, state }: { projectId: number; state: IssueState }) {
  const user = useUser();
  const location = useLocation();
  const [searchParams] = useSearchParams();
  const sort = parseSort(searchParams.get("sort"));
  const page = parsePage(searchParams.get("page"));
  const q = searchParams.get("q") ?? "";

  const project = useLoad(() => projectsApi.projectSummary({ params: { projectId } }), [projectId]);
  const projectData = project.data ?? null;
  useSetNavProject(projectData);
  useTitle(["Issues", projectData?.name]);

  const list = useLoad(
    () =>
      issuesApi.projectIssues({
        params: { projectId },
        queries: { state, sort, offset: (page - 1) * PAGE_SIZE, limit: PAGE_SIZE, ...(q ? { q } : {}) }
      }),
    [projectId, state, sort, page, q]
  );

  if (project.state !== "loaded") {
    return <Load loaded={project}>{() => null}</Load>;
  }
  const latestRelease = project.data.has_releases ? project.data.latest_release : null;

  return (
    <>
      <ProjectWarnings warnings={project.data.warnings} />
      <Load loaded={list}>
        {({ items }) => (
          <IssueList
            key={`${location.pathname}${location.search}`}
            heading={`Issues · ${project.data.name}`}
            state={state}
            tabPath={(tab) => routes.issues(projectId, tab)}
            sort={sort}
            page={page}
            search
            items={items}
            latestRelease={latestRelease}
            noOpenIssuesHint={
              project.data.digested_event_count === 0 && (
                <>
                  {" "}
                  This might mean you have not yet{" "}
                  <Link className="text-cyan-500 dark:text-cyan-300 font-bold" to={routes.projectSdkSetup(projectId)}>
                    set up your SDK
                  </Link>
                  .
                </>
              )
            }
            footer={<ProjectIcons projectId={projectId} userId={user.id} isAdmin={project.data.is_admin} />}
            onAction={async (action, issueIds) => {
              const result = await issuesApi.projectIssueActions(
                { action, issue_ids: issueIds },
                { params: { projectId } }
              );
              return result.unapplied_issue_ids;
            }}
            reload={list.reload}
          />
        )}
      </Load>
    </>
  );
}
