import { useLocation, useSearchParams } from "react-router";

import Load from "common/components/Load";
import { routes, type IssueState } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { useTitle } from "common/hooks/useTitle";
import IssueList, { parsePage, parseSort } from "modules/issues/components/IssueList";
import { PAGE_SIZE } from "modules/issues/components/Pagination";
import { issuesApi } from "services/issues";

/**
 * Upstream's `global_issue_list`: the issues of every project you have issue access to, in a tab (`state`), sorted
 * and paged; no search, and no release-specific actions.
 */
export default function GlobalIssueList({ state }: { state: IssueState }) {
  const location = useLocation();
  const [searchParams] = useSearchParams();
  const sort = parseSort(searchParams.get("sort"));
  const page = parsePage(searchParams.get("page"));
  useTitle("Issues");

  const list = useLoad(
    () => issuesApi.globalIssues({ queries: { state, sort, offset: (page - 1) * PAGE_SIZE, limit: PAGE_SIZE } }),
    [state, sort, page]
  );

  return (
    <Load loaded={list}>
      {({ items }) => (
        <IssueList
          key={`${location.pathname}${location.search}`}
          heading="Issues"
          state={state}
          tabPath={(tab) => routes.globalIssues(tab)}
          sort={sort}
          page={page}
          search={false}
          items={items}
          latestRelease={null}
          onAction={async (action, issueIds) => {
            const result = await issuesApi.globalIssueActions({ action, issue_ids: issueIds });
            return result.unapplied_issue_ids;
          }}
          reload={list.reload}
        />
      )}
    </Load>
  );
}
