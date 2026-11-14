import { useState, type ReactNode } from "react";
import { Link, useSearchParams } from "react-router";

import SearchInput from "common/components/SearchInput";
import type { IssueState } from "common/constants/routes";
import { apiError } from "services/http";
import type { IssueListItem } from "services/issues";

import ActionButtons from "./ActionButtons";
import DeleteModal from "./DeleteModal";
import IssueRow, { checkboxClass } from "./IssueRow";
import Pagination from "./Pagination";

export type Sort = "last_seen" | "events";

/** `_get_issue_list_sort`: unknown sorts fall back to last seen. */
export function parseSort(sort: string | null): Sort {
  return sort === "events" ? "events" : "last_seen";
}

/** `paginator.get_page`: anything but a positive number is the first page. */
export function parsePage(page: string | null): number {
  const n = Number(page);
  return Number.isInteger(n) && n >= 1 ? n : 1;
}

const TABS: { state: IssueState; label: string }[] = [
  { state: "open", label: "Open" },
  { state: "unresolved", label: "Unresolved" },
  { state: "muted", label: "Muted" },
  { state: "resolved", label: "Resolved" },
  { state: "all", label: "All" }
];

type Props = {
  /** "Issues", or "Issues · <project>". */
  heading: string;
  state: IssueState;
  /** The list's path for a tab. */
  tabPath: (state: IssueState) => string;
  sort: Sort;
  page: number;
  /** Project lists only. */
  search: boolean;
  items: IssueListItem[];
  /** The project's latest release when it has releases; null on the global list. */
  latestRelease: string | null;
  /** Appended to the "no open issues" text (the SDK setup hint for a project without events). */
  noOpenIssuesHint?: ReactNode;
  /** Right of the pagination (the project icons). */
  footer?: ReactNode;
  /** Applies a bulk action; resolves to the ids it didn't apply to. */
  onAction: (action: string, issueIds: string[]) => Promise<string[]>;
  /** Reloads the list (after an action). */
  reload: () => void;
};

/** Upstream's `issues/issue_list.html`, for a project's and for the global issue list. */
export default function IssueList(props: Props) {
  const { heading, state, tabPath, sort, page, search, items, latestRelease, onAction, reload } = props;
  const [searchParams, setSearchParams] = useSearchParams();
  const q = search ? (searchParams.get("q") ?? "") : "";

  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set());
  const [mainChecked, setMainChecked] = useState(false);
  const [unapplied, setUnapplied] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [busy, setBusy] = useState(false);

  // `{% querystring sort=sort page=None %}`
  function tabLink(tabState: IssueState) {
    const params = new URLSearchParams(searchParams);
    params.set("sort", sort);
    params.delete("page");
    return `${tabPath(tabState)}?${params.toString()}`;
  }

  // the issue links: `{% querystring page=None sort=None %}`
  const linkParams = new URLSearchParams(searchParams);
  linkParams.delete("page");
  linkParams.delete("sort");
  const linkSearch = linkParams.size ? `?${linkParams.toString()}` : "";

  // the sort <select> submits its form: `sort` (and the search input's `q`)
  function changeSort(next: string) {
    const params = new URLSearchParams({ sort: next });
    if (search) {
      params.set("q", q);
    }
    setSearchParams(params);
  }

  // `issue_list.js`: the main checkbox sets all; the others set it when they're all (un)checked
  function toggleMain() {
    if (!items.length) {
      return;
    }
    const checked = !mainChecked;
    setMainChecked(checked);
    setSelected(checked ? new Set(items.map((issue) => issue.id)) : new Set());
  }

  function toggle(issueId: string) {
    const next = new Set(selected);
    if (next.has(issueId)) {
      next.delete(issueId);
    } else {
      next.add(issueId);
    }
    setSelected(next);
    if (items.every((issue) => next.has(issue.id))) {
      setMainChecked(true);
    }
    if (items.every((issue) => !next.has(issue.id))) {
      setMainChecked(false);
    }
  }

  // like upstream's POST: issues the action didn't apply to stay checked, with a message
  async function act(action: string) {
    if (busy) {
      return;
    }
    setBusy(true);
    try {
      const unappliedIds = await onAction(
        action,
        items.filter((issue) => selected.has(issue.id)).map((issue) => issue.id)
      );
      setSelected(new Set(unappliedIds));
      setMainChecked(false);
      setUnapplied(unappliedIds.length > 0);
      setFailure(null);
      reload();
    } catch (error) {
      setFailure(apiError(error)?.detail ?? String(error));
    } finally {
      setBusy(false);
      setConfirmDelete(false);
    }
  }

  return (
    <>
      {confirmDelete && <DeleteModal onCancel={() => setConfirmDelete(false)} onConfirm={() => void act("delete")} />}

      <div className="m-4">
        <h1 className="text-4xl mt-4 font-bold">{heading}</h1>

        {unapplied && (
          <div className="bg-red-100 dark:bg-red-900 w-full mt-2 mb-2 p-4 border-red-800 dark:border-red-700 border-2">
            The chosen action is not applicable to all selected issues. Issues for which it has not been applied have
            been left with checkboxes checked so that you can try again with another action.
          </div>
        )}
        {failure && (
          <div className="bg-red-100 dark:bg-red-900 w-full mt-2 mb-2 p-4 border-red-800 dark:border-red-700 border-2">
            {failure}
          </div>
        )}

        <div className="flex flex-wrap bg-slate-50 dark:bg-slate-800 border-b-2 mt-4 items-end">
          <div className="flex">
            {TABS.map((tab) => (
              <Link key={tab.state} to={tabLink(tab.state)}>
                <div
                  className={`p-4 font-bold hover:bg-slate-200 dark:hover:bg-slate-800 ${
                    tab.state === state
                      ? "text-cyan-500 dark:text-cyan-300 border-cyan-500 border-b-4"
                      : "text-slate-500 dark:text-slate-300 hover:border-b-4 hover:border-slate-400"
                  }`}
                >
                  {tab.label}
                </div>
              </Link>
            ))}
          </div>
          <div className="ml-auto p-2">
            <div className="flex flex-wrap items-center justify-end gap-2">
              <select
                id="issue-sort"
                name="sort"
                aria-label="Sort issues"
                value={sort}
                onChange={(event) => changeSort(event.target.value)}
                className="bg-slate-50 dark:bg-slate-800 focus:border-cyan-500 dark:focus:border-cyan-400 focus:ring-cyan-200 dark:focus:ring-cyan-700 rounded-md"
              >
                <option value="last_seen">Last seen</option>
                <option value="events">Most events</option>
              </select>
              {search && <SearchInput key={q} placeholder="Search issues..." />}
            </div>
          </div>
        </div>

        <div>
          <table className="w-full">
            <thead>
              <tr className="bg-white dark:bg-slate-900 border-slate-300 dark:border-slate-600 border-l-2 border-r-2">
                <td>
                  <div
                    className="m-1 rounded-full hover:bg-slate-100 dark:hover:bg-slate-700 cursor-pointer"
                    onClick={toggleMain}
                  >
                    {items.length > 0 && (
                      <input
                        type="checkbox"
                        checked={mainChecked}
                        className={`${checkboxClass} js-main-checkbox`}
                        onClick={(event) => event.stopPropagation()}
                        onChange={toggleMain}
                      />
                    )}
                  </div>
                </td>
                <td className="w-full ml-0 pb-4 pt-4 pr-4 flex">
                  <div className="ml-auto">
                    <ActionButtons
                      latestRelease={latestRelease}
                      disableResolve={state === "resolved"}
                      disableMute={state === "resolved" || state === "muted"}
                      disableUnmute={state === "resolved" || state === "open"}
                      onAction={(action) => void act(action)}
                      onDelete={() => setConfirmDelete(true)}
                    />
                  </div>
                </td>
              </tr>
            </thead>
            <tbody>
              {items.map((issue) => (
                <IssueRow
                  key={issue.id}
                  issue={issue}
                  linkSearch={linkSearch}
                  checked={selected.has(issue.id)}
                  onToggle={() => toggle(issue.id)}
                />
              ))}
              {!items.length && (
                <tr className="bg-slate-50 dark:bg-slate-800 border-slate-300 dark:border-slate-600 border-2 ">
                  <td></td>
                  <td className="w-full ml-0 pb-4 pt-4 pr-4 text-center">
                    <div className="p-4 text-xl font-bold text-slate-800 dark:text-slate-100">
                      {q ? (
                        `No ${state} issues found for "${q}"`
                      ) : state === "open" ? (
                        <>Congratulations! You have no open issues.{props.noOpenIssuesHint}</>
                      ) : (
                        `No ${state} issues found.`
                      )}
                    </div>
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>

        <div className="flex mt-4">
          <Pagination page={page} count={items.length} />
          {props.footer}
        </div>
      </div>
    </>
  );
}
