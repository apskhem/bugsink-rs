import { Link } from "react-router";

import CompactSparkline from "common/components/CompactSparkline";
import { routes } from "common/constants/routes";
import { formatNumber } from "common/utils/format";
import { formatShortDate, friendlyId, issueTitle, truncateChars } from "modules/issues/utils";
import type { IssueListItem } from "services/issues";

import { MutedIcon, ResolvedIcon } from "./icons";

export const checkboxClass =
  "dark:bg-slate-900 checked:dark:bg-cyan-500 border-cyan-800 dark:border-cyan-400 text-cyan-500 dark:text-cyan-300 focus:ring-cyan-200 dark:focus:ring-cyan-700 m-4 cursor-pointer";

type Props = {
  issue: IssueListItem;
  /** The query string the issue link carries (upstream keeps `q`, dropping `page` and `sort`). */
  linkSearch: string;
  checked: boolean;
  onToggle: () => void;
};

export default function IssueRow({ issue, linkSearch, checked, onToggle }: Props) {
  return (
    <tr className="bg-slate-50 dark:bg-slate-800 border-slate-300 dark:border-slate-600 border-2 ">
      <td>
        <div className="m-1 rounded-full hover:bg-slate-100 dark:hover:bg-slate-700 cursor-pointer" onClick={onToggle}>
          <input
            type="checkbox"
            checked={checked}
            name="issue_ids[]"
            value={issue.id}
            className={`${checkboxClass} js-issue-checkbox`}
            onClick={(event) => event.stopPropagation()}
            onChange={onToggle}
          />
        </div>
      </td>
      <td className="w-full ml-0 pb-4 pt-4 pr-4">
        <div className="flex items-end gap-4">
          <div className="min-w-0 flex-1">
            <div>
              <Link
                to={`${routes.event(issue.id, "last")}${linkSearch}`}
                className={`text-cyan-500 dark:text-cyan-300 fill-cyan-500 font-bold ${issue.is_resolved ? "italic" : ""}`}
              >
                {issue.is_resolved && <ResolvedIcon />}
                {issue.is_muted && (
                  <>
                    <MutedIcon />
                    &nbsp;&nbsp;
                  </>
                )}
                {truncateChars(issueTitle(issue.calculated_type, issue.calculated_value), 100)}
              </Link>
            </div>
            <div className="text-sm">
              from <b>{formatShortDate(issue.first_seen)}</b> | last <b>{formatShortDate(issue.last_seen)}</b> |{" "}
              <span className="font-bold text-slate-600 dark:text-slate-300">
                {friendlyId(issue.project_slug, issue.digest_order)}
              </span>{" "}
              | with <b>{formatNumber(issue.digested_event_count)}</b> events{" "}
              {issue.digested_event_count !== issue.stored_event_count && (
                <span className="text-xs">({formatNumber(issue.stored_event_count)}&thinsp;av)</span>
              )}{" "}
              {issue.transaction && (
                <>
                  | <span className="font-bold">{issue.transaction} </span>
                </>
              )}
            </div>
          </div>
          <div className="flex-none">
            <CompactSparkline counts={issue.sparkline} />
          </div>
        </div>
      </td>
    </tr>
  );
}
