import { useReducer } from "react";
import { useParams } from "react-router";

import { LoadError, NotFound } from "common/components/ErrorPage";
import type { EventTab } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { formatNumber } from "common/utils/format";
import Breadcrumbs from "modules/issue/components/Breadcrumbs";
import Event404 from "modules/issue/components/Event404";
import EventDetails from "modules/issue/components/EventDetails";
import EventNav from "modules/issue/components/EventNav";
import IssueLayout, { LOG_MESSAGE_TYPE, type IssueTab } from "modules/issue/components/IssueLayout";
import Sparkline from "modules/issue/components/Sparkline";
import Stacktrace from "modules/issue/components/Stacktrace";
import { useQ } from "modules/issue/hooks";
import { dateTime } from "modules/issue/utils/dates";
import { issueSparkline } from "modules/issue/utils/sparkline";
import { isStatus } from "services/http";
import { issueApi, type EventPage as EventPageData, type Issue } from "services/issue";

const ISSUE_TAB: Record<EventTab, IssueTab> = {
  stacktrace: "stacktrace",
  details: "event-details",
  breadcrumbs: "breadcrumbs"
};

/** The issue's hourly counts as upstream's clickable sparkline, marking the event shown. */
function IssueSparkline({ issueId, q, activeEventAt }: { issueId: string; q: string; activeEventAt: Date | null }) {
  const counts = useLoad(
    async () =>
      issueSparkline(
        await issueApi.counts({ params: { issueId }, queries: q ? { q } : {} }),
        new Date(),
        activeEventAt
      ),
    [issueId, q, activeEventAt?.getTime()]
  );
  return counts.state === "loaded" ? <Sparkline issueId={issueId} variants={counts.data} /> : null;
}

/** The "digested at" of the event (for the sparkline's marker), from the details' key info. */
function digestedAt(page: EventPageData): Date {
  const value = page.display?.key_info.find(([key]) => key === "digested at")?.[1];
  const date = typeof value === "string" ? new Date(value) : null;
  return date && !Number.isNaN(date.getTime()) ? date : new Date(page.event.ingested_at);
}

type ContentProps = { issue: Issue; tab: EventTab; q: string; version: number; page: EventPageData };

function EventContent({ issue, tab, q, page }: ContentProps) {
  const { event, nav, display } = page;
  const eventLine = `${dateTime(event.ingested_at)} (Event ${formatNumber(event.digest_order)} of ${formatNumber(issue.digested_event_count)} total${q ? ` — ${formatNumber(nav.count)} found by search` : ""})`;
  const navigation = (
    <EventNav
      issueId={issue.id}
      tab={tab}
      digestOrder={event.digest_order}
      hasPrev={nav.prev !== null}
      hasNext={nav.next !== null}
    />
  );

  if (display === null) {
    // the payload is in upstream's file-based event storage, which this server can't read
    return (
      <>
        <div className="flex">
          <div className="overflow-hidden">
            <div className="italic">{eventLine}</div>
          </div>
          <div className="ml-auto flex-none">
            <div className="flex place-content-end">{navigation}</div>
          </div>
        </div>
        <div className="mt-6 mb-6 italic">The data of this event is not available.</div>
      </>
    );
  }

  switch (tab) {
    case "stacktrace":
      return (
        <Stacktrace
          key={event.id}
          display={display}
          platform={event.platform}
          title={display.title}
          isLogMessage={event.calculated_type === LOG_MESSAGE_TYPE}
          eventLine={eventLine}
          nav={navigation}
        />
      );
    case "details":
      return (
        <EventDetails
          display={display}
          data={event.data}
          tags={page.tags}
          projectId={issue.project_id}
          q={q}
          eventLine={eventLine}
          nav={navigation}
          sparkline={<IssueSparkline issueId={issue.id} q={q} activeEventAt={digestedAt(page)} />}
        />
      );
    case "breadcrumbs":
      return <Breadcrumbs breadcrumbs={display.breadcrumbs} eventLine={eventLine} nav={navigation} />;
  }
}

/**
 * Upstream's event pages (`issue_event_stacktrace`, `issue_event_details`, `issue_event_breadcrumbs`): the event is a
 * digest order, an event id, "first" or "last" (within the search), or (`nav`) the previous/next one from a digest
 * order.
 */
export default function EventPage({ tab, nav }: { tab: EventTab; nav?: "prev" | "next" }) {
  const { issueId = "", event: selector = "" } = useParams();
  const q = useQ();
  const [version, bump] = useReducer((n: number) => n + 1, 0);
  const validSelector = nav === undefined || /^\d+$/.test(selector);

  const loaded = useLoad(() => {
    const queries = q ? { q } : {};
    if (!validSelector) {
      return Promise.resolve(null);
    }
    return nav
      ? issueApi.eventNav({ params: { issueId, digestOrder: selector, nav }, queries })
      : issueApi.event({ params: { issueId, selector }, queries });
  }, [issueId, selector, nav, q, version, validSelector]);

  if (!validSelector) {
    return <NotFound />;
  }
  const page = loaded.data ?? null;

  return (
    <IssueLayout
      issueId={issueId}
      tab={ISSUE_TAB[tab]}
      version={version}
      onChanged={bump}
      event={page?.event ?? null}
      requestRepr={page?.display?.request_repr ?? ""}
    >
      {(issue) => {
        if (loaded.state === "failed") {
          return isStatus(loaded.error, 404) ? (
            <Event404 issue={issue} tab={tab} q={q} version={version} />
          ) : (
            <LoadError error={loaded.error} />
          );
        }
        return page && <EventContent issue={issue} tab={tab} q={q} version={version} page={page} />;
      }}
    </IssueLayout>
  );
}
