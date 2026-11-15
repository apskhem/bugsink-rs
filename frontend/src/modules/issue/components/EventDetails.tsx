import type { ReactNode } from "react";
import { Link } from "react-router";

import { Linebreaks, TimestampWithMillis } from "modules/issue/components/Text";
import { asRecord, isRecord, truthy } from "modules/issue/utils/python";
import { tagDrilldownHref } from "modules/issue/utils/tags";
import type { EventDisplay } from "services/issue";

const DOTTED = "border-b-2 border-dotted border-slate-300 dark:border-slate-600";

/** One key/value row of the details tables. */
function Row({
  first,
  last,
  label,
  children
}: {
  first: boolean;
  last: boolean;
  label: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className={`flex flex-wrap ${first ? "border-slate-300 dark:border-slate-600 border-t-2" : ""}`}>
      <div className={`w-1/4 break-words ${!last ? DOTTED : ""}`}>{label}</div>
      <div className={`w-3/4 break-words ${!last ? ` ${DOTTED}` : ""} font-mono`}>{children}</div>
    </div>
  );
}

/** A section of label/value pairs, each value through `linebreaks`. */
function Section({ id, title, items }: { id: string; title: string; items: [string, ReactNode][] }) {
  return (
    <>
      <h1 id={id} className="text-2xl font-bold mt-4">
        {title}
      </h1>
      <div className="mb-6">
        {items.map(([key, value], i) => (
          <Row key={`${key}-${i}`} first={i === 0} last={i === items.length - 1} label={key}>
            {value}
          </Row>
        ))}
      </div>
    </>
  );
}

const linebreaks = (pairs: [string, unknown][]): [string, ReactNode][] =>
  pairs.map(([key, value]) => [key, <Linebreaks value={value} />]);

/** `|items`: a dict's items; nothing for anything else. */
const items = (value: unknown): [string, unknown][] => (isRecord(value) ? Object.entries(value) : []);

/** Key info's timestamps go through `timestamp_with_millis` upstream. */
const TIMESTAMP_KEYS = ["timestamp", "ingested at", "digested at"];

type Props = {
  display: EventDisplay;
  /** The event's payload. */
  data: unknown;
  /** The tags stored for the event, by key. */
  tags: [string, string][];
  projectId: number;
  q: string;
  eventLine: ReactNode;
  nav: ReactNode;
  sparkline: ReactNode;
};

/** Upstream's `issues/event_details.html`. */
export default function EventDetails({ display, data, tags, projectId, q, eventLine, nav, sparkline }: Props) {
  const parsed = asRecord(data) ?? {};
  const user = parsed["user"];
  const request = parsed["request"];
  const requestRecord = asRecord(request) ?? {};
  const requestItems = items(request);
  const headers = requestRecord["headers"];
  const env = requestRecord["env"];
  const contexts = items(display.contexts);
  const modules = items(parsed["modules"]).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));

  return (
    <>
      <div className="flex">
        <div className="overflow-hidden">
          <div className="italic text-ellipsis whitespace-nowrap overflow-hidden">{eventLine}</div>
        </div>
        <div className="ml-auto flex-none">
          <div className="flex place-content-end">{nav}</div>
        </div>
      </div>

      {sparkline}

      <Section
        id="key-info"
        title="Key info"
        items={display.key_info.map(([key, value]) => [
          key,
          TIMESTAMP_KEYS.includes(key) ? (
            <p>
              <TimestampWithMillis value={value} />
            </p>
          ) : (
            <Linebreaks value={value} />
          )
        ])}
      />

      {display.logentry_info.length > 0 && (
        <Section id="logentry" title="Log Entry" items={linebreaks(display.logentry_info)} />
      )}

      {display.deployment_info.length > 0 && (
        <Section id="deployment" title="Deployment" items={linebreaks(display.deployment_info)} />
      )}

      {tags.length > 0 && (
        <Section
          id="tags"
          title="Tags"
          items={tags.map(([key, value]) => [
            key,
            <Link
              to={tagDrilldownHref(projectId, q, key, value)}
              className="hover:text-cyan-500 dark:hover:text-cyan-300"
              title="Filter issues by this tag"
            >
              <Linebreaks value={value} />
            </Link>
          ])}
        />
      )}

      {truthy(user) && <Section id="user" title="User" items={linebreaks(items(user))} />}

      {truthy(request) && (
        <>
          <h1 id="request" className="text-2xl font-bold mt-4">
            Request
          </h1>
          <div className="mb-6">
            <div>
              {requestItems.map(([key, value], i) =>
                key === "headers" || key === "env" ? null : (
                  <Row key={key} first={i === 0} last={i === requestItems.length - 1} label={key}>
                    <Linebreaks value={value} />
                  </Row>
                )
              )}
            </div>

            {truthy(headers) && <RequestDict title="REQUEST HEADERS" value={headers} />}
            {truthy(env) && <RequestDict title="REQUEST ENV" value={env} />}
          </div>
        </>
      )}

      {contexts.length > 0 && (
        <>
          <h1 id="runtime" className="text-2xl font-bold mt-4">
            Contexts
          </h1>
          <div className="mb-6">
            {contexts.map(([contextKey, context]) => {
              const contextItems = items(context);
              return (
                <div key={contextKey}>
                  <h3 className="font-bold mt-2">{contextKey.toUpperCase()}</h3>
                  {contextItems.map(([key, value], i) => (
                    <Row key={key} first={i === 0} last={i === contextItems.length - 1} label={key}>
                      <Linebreaks value={value} />
                    </Row>
                  ))}
                </div>
              );
            })}
          </div>
        </>
      )}

      {truthy(parsed["modules"]) && <Section id="modules" title="Modules" items={linebreaks(modules)} />}
      {truthy(parsed["sdk"]) && <Section id="sdk" title="SDK" items={linebreaks(items(parsed["sdk"]))} />}
      {truthy(parsed["extra"]) && <Section id="extra" title="Extra" items={linebreaks(items(parsed["extra"]))} />}
    </>
  );
}

/** The request's headers or env: only a dict is shown (`.items` in the template). */
function RequestDict({ title, value }: { title: string; value: unknown }) {
  const pairs = items(value);
  return (
    <>
      <h3 className="font-bold mt-2">{title}</h3>
      <div>
        {pairs.map(([key, v], i) => (
          <div
            key={key}
            className={`flex flex-wrap ${i === 0 ? "border-slate-300 dark:border-slate-600 border-t-2" : ""} border-dotted`}
          >
            <div className={`w-1/4 break-words ${i < pairs.length - 1 ? DOTTED : ""}`}>{key}</div>
            <div className={`w-3/4 break-words ${i < pairs.length - 1 ? ` ${DOTTED}` : ""} font-mono`}>
              <Linebreaks value={v} />
            </div>
          </div>
        ))}
      </div>
    </>
  );
}
