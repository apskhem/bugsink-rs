import { useReducer } from "react";
import { Link, useLocation, useParams, useSearchParams } from "react-router";

import { LoadError } from "common/components/ErrorPage";
import SearchInput from "common/components/SearchInput";
import { routes } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { formatNumber } from "common/utils/format";
import { NavButton } from "modules/issue/components/EventNav";
import { FirstIcon, LastIcon, NextIcon, PrevIcon } from "modules/issue/components/Icons";
import IssueLayout, { LOG_MESSAGE_TYPE } from "modules/issue/components/IssueLayout";
import { useLastEvent, useQ } from "modules/issue/hooks";
import { dateTimeSeconds, millis } from "modules/issue/utils/dates";
import { issha, shortsha } from "modules/issue/utils/releases";
import { issueApi, type Issue } from "services/issue";

const PAGE_SIZE = 250;

/** Django's `truncatechars`. */
const truncatechars = (s: string, n: number) => (s.length > n ? `${s.slice(0, n - 1)}…` : s);

/** `get_title_for_exception_type_and_value` */
function eventTitle(type: string, value: string): string {
  if (!value) {
    return type;
  }
  const firstLine = value.split(/\r\n|\r|\n/)[0] ?? "";
  return type === LOG_MESSAGE_TYPE ? firstLine : `${type}: ${firstLine}`;
}

type ListProps = { issue: Issue; q: string; count: number; version: number };

function Events({ issue, q, count, version }: ListProps) {
  const [searchParams] = useSearchParams();
  const { search } = useLocation();
  // Django's `Paginator.get_page`: not a number, the first page; beyond the end, the last
  const numPages = Math.max(1, Math.ceil(count / PAGE_SIZE));
  const requested = Number.parseInt(searchParams.get("page") ?? "", 10);
  const page = Number.isNaN(requested) || requested < 1 ? 1 : Math.min(requested, numPages);

  const events = useLoad(
    () =>
      issueApi.events({
        params: { issueId: issue.id },
        queries: { ...(q ? { q } : {}), offset: (page - 1) * PAGE_SIZE, limit: PAGE_SIZE }
      }),
    [issue.id, q, page, version]
  );
  if (events.state === "failed") {
    return <LoadError error={events.error} />;
  }
  const items = events.data?.items ?? [];

  const pageTo = (n: number) => {
    const params = new URLSearchParams(searchParams);
    params.set("page", String(n));
    return `?${params.toString()}`;
  };
  const startIndex = count === 0 ? 0 : (page - 1) * PAGE_SIZE + 1;
  const endIndex = page === numPages ? count : page * PAGE_SIZE;
  const { stored_event_count: stored, digested_event_count: digested } = issue;

  return (
    <>
      <div className="flex">
        <div className="overflow-hidden">
          <div className="italic">
            Showing {formatNumber(startIndex)} - {formatNumber(endIndex)} of{" "}
            {count === stored && stored === digested
              ? `${formatNumber(count)} total events.`
              : count === stored
                ? `${formatNumber(count)} available events (${formatNumber(digested)} total observed).`
                : stored === digested
                  ? `${formatNumber(count)} events found (${formatNumber(digested)} total observed).`
                  : `${formatNumber(count)} events found (${formatNumber(stored)} available, ${formatNumber(digested)} total observed).`}
          </div>
        </div>

        <div className="ml-auto flex-none">
          <div className="flex place-content-end">
            <SearchInput placeholder="search..." containerClass="mr-2" />
            <NavButton to={page > 1 ? pageTo(1) : null} title="First page">
              <FirstIcon />
            </NavButton>
            <NavButton to={page > 1 ? pageTo(page - 1) : null} title="Previous page">
              <PrevIcon />
            </NavButton>
            <NavButton to={page < numPages ? pageTo(page + 1) : null} title="Next page">
              <NextIcon />
            </NavButton>
            <NavButton to={page < numPages ? pageTo(numPages) : null} title="Last page">
              <LastIcon />
            </NavButton>
          </div>
        </div>
      </div>

      <div className="pt-4">
        <table className="w-full">
          <thead>
            <tr>
              <td className="p-4 align-top text-slate-800 dark:text-slate-100 font-bold">#</td>
              <td className="p-4 align-top text-slate-800 dark:text-slate-100 font-bold">ID</td>
              <td className="p-4 align-top text-slate-800 dark:text-slate-100 font-bold">Timestamp</td>
              <td className="p-4 w-full align-top text-slate-800 dark:text-slate-100 font-bold">Title</td>
              <td className="p-4 align-top text-slate-800 dark:text-slate-100 font-bold">Release</td>
              <td className="p-4 align-top text-slate-800 dark:text-slate-100 font-bold">Environment</td>
            </tr>
          </thead>
          <tbody>
            {items.map((event) => {
              const timestamp = new Date(event.timestamp);
              return (
                <tr key={event.id} className="border-slate-200 dark:border-slate-700 border-2 ">
                  <td className="p-4 font-bold text-slate-500 dark:text-slate-300 align-top">
                    <Link to={`${routes.event(issue.id, event.id)}${search}`}>{event.digest_order}</Link>
                  </td>
                  <td className="p-4 font-bold text-slate-500 dark:text-slate-300 align-top">
                    <Link to={`${routes.event(issue.id, event.id)}${search}`}>{truncatechars(event.id, 9)}</Link>
                  </td>
                  <td className="p-4 font-mono whitespace-nowrap align-top">
                    {dateTimeSeconds(timestamp)}.<span className="text-xs">{millis(timestamp)}</span>
                  </td>
                  <td className="w-full p-4 font-mono align-top">
                    {eventTitle(event.calculated_type, event.calculated_value)}
                  </td>
                  <td className="p-4 font-mono align-top">
                    <span className={issha(event.release) ? "font-mono" : undefined}>{shortsha(event.release)}</span>
                  </td>
                  <td className="p-4 font-mono align-top">{truncatechars(event.environment, 30)}</td>
                </tr>
              );
            })}
            {events.state === "loaded" && items.length === 0 && (
              <tr>
                <td colSpan={6} className="p-4 text-slate-800 dark:text-slate-100 italic">
                  No events found{q && ` for "${q}"`}.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </>
  );
}

/** Upstream's `issues/event_list.html`: the issue's events, newest first, 250 per page. */
export default function EventList() {
  const { issueId = "" } = useParams();
  const q = useQ();
  const [version, bump] = useReducer((n: number) => n + 1, 0);
  const last = useLastEvent(issueId, q, version);

  return (
    <IssueLayout
      issueId={issueId}
      tab="event-list"
      version={version}
      onChanged={bump}
      event={null}
      requestRepr={last.data?.display?.request_repr ?? ""}
    >
      {(issue) => {
        if (q && last.state === "loading") {
          return null;
        }
        // with a search, its count (the last matching event's navigation counts the matches); else the stored events
        const count = q ? (last.data?.nav.count ?? 0) : issue.stored_event_count;
        return <Events issue={issue} q={q} count={count} version={version} />;
      }}
    </IssueLayout>
  );
}
