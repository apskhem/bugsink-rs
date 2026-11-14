import { useState, type ReactNode } from "react";
import { Link, useLocation, useNavigate, useSearchParams } from "react-router";

import { routes, type EventTab } from "common/constants/routes";
import { FirstIcon, LastIcon, NextIcon, PrevIcon } from "modules/issue/components/Icons";

/** One of the first/previous/next/last buttons: a link, or greyed out without one. */
export function NavButton({ to, title, children }: { to: string | null; title: string; children: ReactNode }) {
  if (to === null) {
    return (
      <div
        className="font-bold text-slate-300 dark:text-slate-600 border-slate-300 dark:border-slate-600 pl-4 pr-4 pb-1 pt-1 mr-2 border-2 rounded-md inline-flex items-center justify-center"
        title={title}
      >
        {children}
      </div>
    );
  }
  return (
    <Link
      to={to}
      className="font-bold text-slate-500 dark:text-slate-300 border-slate-300 dark:border-slate-600 pl-4 pr-4 pb-1 pt-1 mr-2 border-2 rounded-md hover:bg-slate-200 dark:hover:bg-slate-800 active:ring-3 inline-flex items-center justify-center"
      title={title}
    >
      {children}
    </Link>
  );
}

/**
 * `theme/_search_input.html` in the event navigation's form, which searches from the last matching event
 * (`nav="last"`: when doing a new search on an event page, you want the most recent matching event to show up).
 */
export function EventSearchInput({ issueId, tab }: { issueId: string; tab: EventTab }) {
  const navigate = useNavigate();
  const location = useLocation();
  const [searchParams] = useSearchParams();
  const q = searchParams.get("q") ?? "";
  const [value, setValue] = useState(q);
  const [shownQ, setShownQ] = useState(q);
  if (shownQ !== q) {
    // the search changed by navigating (e.g. the ×)
    setShownQ(q);
    setValue(q);
  }

  return (
    <form
      onSubmit={(event) => {
        event.preventDefault();
        const query = new URLSearchParams({ q: value });
        void navigate(`${routes.event(issueId, "last", tab)}?${query.toString()}`);
      }}
    >
      <div className="relative inline-block mr-2">
        <input
          type="text"
          name="q"
          value={value}
          placeholder="search..."
          onChange={(event) => setValue(event.target.value)}
          className="bg-slate-50 dark:bg-slate-800 focus:border-cyan-500 dark:focus:border-cyan-400 focus:ring-cyan-200 dark:focus:ring-cyan-700 rounded-md pr-9"
        />
        {q && (
          <Link
            to={(() => {
              const params = new URLSearchParams(searchParams);
              params.delete("q");
              params.delete("page");
              const rest = params.toString();
              return `${location.pathname}${rest ? `?${rest}` : ""}`;
            })()}
            aria-label="Clear search"
            title="Clear search"
            className="absolute inset-y-0 right-0 flex items-center px-3 text-xl leading-none text-slate-400 hover:text-slate-700 dark:hover:text-slate-300"
          >
            &times;
          </Link>
        )}
      </div>
    </form>
  );
}

type Props = {
  issueId: string;
  tab: EventTab;
  /** The current event's digest order, and whether there are matching events before/after it. */
  digestOrder: number;
  hasPrev: boolean;
  hasNext: boolean;
};

/** Upstream's `issues/_event_nav.html`: search, then first/previous/next/last (within the search). */
export default function EventNav({ issueId, tab, digestOrder, hasPrev, hasNext }: Props) {
  const { search } = useLocation();
  const to = (event: string | number) => `${routes.event(issueId, event, tab)}${search}`;
  return (
    <>
      <EventSearchInput issueId={issueId} tab={tab} />
      <NavButton to={hasPrev ? to("first") : null} title="First event">
        <FirstIcon />
      </NavButton>
      <NavButton to={hasPrev ? to(`${digestOrder}/prev`) : null} title="Previous event">
        <PrevIcon />
      </NavButton>
      <NavButton to={hasNext ? to(`${digestOrder}/next`) : null} title="Next event">
        <NextIcon />
      </NavButton>
      <NavButton to={hasNext ? to("last") : null} title="Last event">
        <LastIcon />
      </NavButton>
    </>
  );
}
