import type { ReactNode } from "react";
import { Link, useLocation } from "react-router";

import Load from "common/components/Load";
import { routes } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { useTitle } from "common/hooks/useTitle";
import { useSetNavProject } from "common/layouts/NavProject";
import { formatNumber } from "common/utils/format";
import IssueActions from "modules/issue/components/IssueActions";
import { useProjectName, useQ } from "modules/issue/hooks";
import { dateTime } from "modules/issue/utils/dates";
import { issha, shortsha, summarizedReleases } from "modules/issue/utils/releases";
import { tagDrilldownHref } from "modules/issue/utils/tags";
import { apiError } from "services/http";
import { issueApi, VolumeBasedCondition, type Event, type Issue, type KeyTags } from "services/issue";

export type IssueTab = "stacktrace" | "event-details" | "breadcrumbs" | "event-list" | "tags" | "grouping" | "history";

export const LOG_MESSAGE_TYPE = "Log Message";

type Props = {
  issueId: string;
  tab: IssueTab;
  /** Bumped after an action, to load the issue again. */
  version: number;
  onChanged: () => void;
  /** On the event pages: the event shown (upstream's `is_event_page`). */
  event: Event | null;
  /** "METHOD url" of the event's request (or the last matching event's, on the other tabs). */
  requestRepr: string;
  children: (issue: Issue) => ReactNode;
};

/** Upstream's `issues/base.html`: header with actions, the tabs around the page's content, and the side boxes. */
export default function IssueLayout(props: Props) {
  const { issueId, version } = props;
  const loaded = useLoad(() => issueApi.issue({ params: { issueId } }), [issueId, version]);
  return <Load loaded={loaded}>{(issue) => <IssuePage {...props} issue={issue} />}</Load>;
}

function IssuePage({ issue, tab, version, onChanged, event, requestRepr, children }: Props & { issue: Issue }) {
  const { search } = useLocation();
  const q = useQ();
  const projectName = useProjectName(issue.project_id);
  useSetNavProject(projectName === null ? null : { id: issue.project_id, name: projectName });
  useTitle(issue.title);

  const tags = useLoad(
    () => (tab === "tags" ? Promise.resolve([]) : issueApi.tags({ params: { issueId: issue.id } })),
    [issue.id, tab, version]
  );

  async function act(action: string) {
    try {
      await issueApi.action({ action }, { params: { issueId: issue.id } });
    } catch (error) {
      window.alert(apiError(error)?.detail ?? String(error));
    }
    onChanged();
  }

  const isLogMessage = issue.calculated_type === LOG_MESSAGE_TYPE;
  const eventPart = event ? event.id : "last";
  const tabs: { key: IssueTab; label: string; to: string }[] = [
    { key: "stacktrace", label: "Stacktrace", to: routes.event(issue.id, eventPart) },
    { key: "event-details", label: "Event Details", to: routes.event(issue.id, eventPart, "details") },
    { key: "breadcrumbs", label: "Breadcrumbs", to: routes.event(issue.id, eventPart, "breadcrumbs") },
    { key: "event-list", label: "Event List", to: routes.issueEvents(issue.id) },
    { key: "tags", label: "Tags", to: routes.issueTags(issue.id) },
    { key: "grouping", label: "Grouping", to: routes.issueGrouping(issue.id) },
    { key: "history", label: "History", to: routes.issueHistory(issue.id) }
  ];

  return (
    <>
      {/* container for the white bit (issue title, buttons) at the top of the page */}
      <div className="m-4 flex flex-col lg:flex-row-reverse">
        <div className="ml-auto flex-none pb-4 lg:pb-0">
          <IssueActions issue={issue} onAction={(action) => void act(action)} />
        </div>

        <div className="overflow-hidden">
          {isLogMessage ? (
            <h1 className="text-4xl font-bold text-ellipsis whitespace-nowrap overflow-hidden pb-1">{issue.title}</h1>
          ) : (
            <>
              <h1 className="text-4xl font-bold text-ellipsis whitespace-nowrap overflow-hidden pb-1">
                {issue.calculated_type}
              </h1>
              <div className="text-xl text-ellipsis whitespace-nowrap overflow-hidden">{issue.calculated_value}</div>
            </>
          )}
          {requestRepr && <div className="italic mt-4">{requestRepr}</div>}
          <div className="text-ellipsis whitespace-nowrap overflow-hidden">
            <span className="font-bold">{issue.last_frame_module || issue.last_frame_filename}</span>
            {issue.last_frame_function && (
              <>
                {" in "}
                <span className="font-bold">{issue.last_frame_function}</span>
              </>
            )}
            {isLogMessage && event?.level && (
              <>
                {(issue.last_frame_module || issue.last_frame_filename || issue.last_frame_function) && " | "}
                <span className="font-bold">{event.level.toUpperCase()}</span>
              </>
            )}
          </div>
        </div>
      </div>

      {/* flex container for the 2 'windows' in this page */}
      <div className="flex items-start">
        {/* the whole of the big tabbed view */}
        <div className="ml-4 mb-4 mr-4 border-2 overflow-x-auto flex-[2_1_96rem]">
          <div className="flex bg-slate-50 dark:bg-slate-800 border-b-2">
            {tabs.map(({ key, label, to }) => (
              <Link key={key} to={`${to}${search}`}>
                <div
                  className={`p-4 font-bold hover:bg-slate-200 dark:hover:bg-slate-800 ${tab === key ? "text-cyan-500 dark:text-cyan-300 border-cyan-500 border-b-4" : "text-slate-500 border-slate-400 hover:border-b-4"}`}
                >
                  {label}
                </div>
              </Link>
            ))}
          </div>

          <div className="m-4">{children(issue)}</div>

          {/* bottom nav bar */}
          <div className="flex p-4 bg-slate-200 dark:bg-slate-800 border-b-2">
            {event && (
              <div>
                Event {formatNumber(event.digest_order)} of {formatNumber(issue.digested_event_count)} which occured at{" "}
                <span className="font-bold">{dateTime(event.ingested_at)}</span>
              </div>
            )}
            <div className="ml-auto pr-4 font-bold text-slate-500 dark:text-slate-300">
              {event && (
                <>
                  <a href={`/events/event/${event.id}/download/`}>Download</a>
                  {" | "}
                  <a href={`/events/event/${event.id}/raw/`}>JSON</a>
                  {" | "}
                  <a href={`/events/event/${event.id}/md/`}>Markdown</a>
                </>
              )}
            </div>
          </div>
        </div>

        {/* RHS container for multiple stacked boxes */}
        <div className="w-128 hidden xl:flex flex-auto flex-col">
          <KeyInfoBox issue={issue} />
          {tags.state === "loaded" && tags.data.length > 0 && (
            <TagsBox issue={issue} tags={tags.data} q={q} allTagsHref={`${routes.issueTags(issue.id)}${search}`} />
          )}
        </div>
      </div>
    </>
  );
}

function Version({ version }: { version: string }) {
  return <span className={issha(version) ? "font-mono" : undefined}>{shortsha(version)}</span>;
}

function MuteState({ issue }: { issue: Issue }) {
  if (issue.unmute_after) {
    return <>Muted until {dateTime(issue.unmute_after)}.</>;
  }
  const parsed = VolumeBasedCondition.array().safeParse(issue.unmute_on_volume_based_conditions);
  const conditions = parsed.success ? parsed.data : [];
  if (conditions.length) {
    return (
      <>
        Muted{" "}
        {conditions.map((vbc, i) => (
          <span key={i}>
            until at least {vbc.volume} events per {vbc.nr_of_periods} {vbc.period}
            {vbc.nr_of_periods !== 1 && "s"}
            {i === conditions.length - 1 ? " occur." : " or "}
          </span>
        ))}
      </>
    );
  }
  return <>Muted (unconditionally).</>;
}

function KeyInfoBox({ issue }: { issue: Issue }) {
  const releases = summarizedReleases(issue.events_at.filter((version) => version !== ""));
  const label = "text-sm font-bold text-slate-500 dark:text-slate-300";
  return (
    <div className="border-2 mb-4 mr-4">
      <div className="font-bold border-b-2">
        <div className="p-4 border-slate-50 dark:border-slate-900 text-slate-500 dark:text-slate-300">
          Issue Key Info
        </div>
      </div>

      <div className="p-4">
        <div className="mb-4">
          <div className={label}>Issue #</div>
          <div>{issue.friendly_id} </div>
        </div>

        <div className="mb-4">
          <div className={label}>State</div>
          <div>
            {issue.is_resolved ? (
              <>
                Resolved
                {issue.fixed_at.map((version, i) => (
                  <span key={version}>
                    {i === 0 && " in"} <Version version={version} />
                    {i < issue.fixed_at.length - 1 && ","}
                  </span>
                ))}
              </>
            ) : issue.is_muted ? (
              <MuteState issue={issue} />
            ) : (
              "Open"
            )}
          </div>
        </div>

        <div className="mb-4">
          <div className={label}>Nr. of events:</div>
          {issue.digested_event_count !== issue.stored_event_count ? (
            <>
              <div>{formatNumber(issue.digested_event_count)} total seen</div>
              <div>{formatNumber(issue.stored_event_count)} available</div>
            </>
          ) : (
            <div>{formatNumber(issue.digested_event_count)}</div>
          )}
        </div>

        {issue.digested_event_count > 1 ? (
          <>
            <div className="mb-4">
              <div className={label}>First seen:</div>
              <div>{dateTime(issue.first_seen)}</div>
            </div>

            <div className="mb-4">
              <div className={label}>Last seen:</div>
              <div>{dateTime(issue.last_seen)}</div>
            </div>
          </>
        ) : (
          <div className="mb-4">
            <div className={label}>Seen at:</div>
            <div>{dateTime(issue.first_seen)}</div>
          </div>
        )}

        {releases.length > 0 && (
          <div className="mb-4">
            <div className={label}>Seen in releases:</div>
            <div>
              {releases.map((item, i) =>
                "hiddenCount" in item ? (
                  <span key={i}>
                    {" "}
                    ... «{item.hiddenCount} more» ...{item.comma && ","}
                  </span>
                ) : (
                  <span key={i}>
                    {" "}
                    <Version version={item.version} />
                    {item.comma && ","}
                  </span>
                )
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

type TagsBoxProps = { issue: Issue; tags: KeyTags[]; q: string; allTagsHref: string };

function TagsBox({ issue, tags, q, allTagsHref }: TagsBoxProps) {
  return (
    <div className="border-2 mb-4 mr-4">
      <div className="font-bold border-b-2">
        <div className="p-4 border-slate-50 dark:border-slate-900 text-slate-500 dark:text-slate-300">Issue Tags</div>
      </div>
      <div className="p-4">
        {tags.map((keyTags) => {
          const values = [...keyTags.values, ...(keyTags.other ? [keyTags.other] : [])];
          return (
            <div key={keyTags.key} className="mb-4">
              <div className="text-sm font-bold text-slate-500 dark:text-slate-300">{keyTags.key}:</div>
              <div>
                {values.map((tag, i) => (
                  <span key={i}>
                    {tag.value === null ? (
                      "..."
                    ) : (
                      <Link
                        to={tagDrilldownHref(issue.project_id, q, keyTags.key, tag.value)}
                        className="hover:text-cyan-500 dark:hover:text-cyan-300"
                        title="Filter issues by this tag"
                      >
                        {tag.value}
                      </Link>
                    )}{" "}
                    <span className="text-xs">({tag.pct}%)</span>
                    {i < values.length - 1 && ", "}
                  </span>
                ))}
              </div>
            </div>
          );
        })}

        <div className="text-sm font-bold italic text-slate-500 dark:text-slate-300">
          <Link to={allTagsHref}>All tags...</Link>
        </div>
      </div>
    </div>
  );
}
