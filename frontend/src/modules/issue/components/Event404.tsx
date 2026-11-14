import { useLocation } from "react-router";

import { routes, type EventTab } from "common/constants/routes";
import { formatNumber } from "common/utils/format";
import { EventSearchInput, NavButton } from "modules/issue/components/EventNav";
import { FirstIcon, LastIcon, NextIcon, PrevIcon } from "modules/issue/components/Icons";
import { useLastEvent } from "modules/issue/hooks";
import type { Issue } from "services/issue";

type Props = { issue: Issue; tab: EventTab; q: string; version: number };

/** Upstream's `issues/event_404.html`: the issue exists, the event doesn't (or none matches the search). */
export default function Event404({ issue, tab, q, version }: Props) {
  const { search } = useLocation();
  // `_event_count`: the matching events with a search, else the stored ones
  const last = useLastEvent(issue.id, q, version);
  if (q && last.state === "loading") {
    return null;
  }
  const count = q ? (last.data?.nav.count ?? 0) : issue.stored_event_count;
  const to = (nav: "first" | "last") => (count ? `${routes.event(issue.id, nav, tab)}${search}` : null);

  return (
    <>
      <div className="flex">
        <div className="overflow-hidden">
          <div className="italic">
            {formatNumber(issue.digested_event_count)} events in total{q && ` — ${count} found by search`}.
          </div>
        </div>

        <div className="ml-auto flex-none">
          <div className="flex place-content-end">
            <EventSearchInput issueId={issue.id} tab={tab} />
            <NavButton to={to("first")} title="First event">
              <FirstIcon />
            </NavButton>
            <NavButton to={null} title="Previous event">
              <PrevIcon />
            </NavButton>
            <NavButton to={null} title="Next event">
              <NextIcon />
            </NavButton>
            <NavButton to={to("last")} title="Last event">
              <LastIcon />
            </NavButton>
          </div>
        </div>
      </div>

      <h1 className="text-2xl font-bold mt-4">{count ? "404: Event missing from Bugsink" : "No Events"}</h1>

      <div className="mb-6">
        {count
          ? "This event cannot be found. It could have been removed manually or as part of the eviction process."
          : q
            ? "No events found for this search."
            : "No events found. They could have been removed manually or as part of the eviction process."}
      </div>
    </>
  );
}
