import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";

import Frames, { setBodyMinHeight, type FrameState } from "modules/issue/components/Frames";
import { asList, asRecord, djStr, truthy, type JsonRecord } from "modules/issue/utils/python";
import type { EventDisplay } from "services/issue";

type Entry = {
  key: string;
  entry: JsonRecord;
  isException: boolean;
  frames: FrameState[];
};

/** The frames of an entry, with upstream's position labels ("raise …", "→ begin", "try…") and markers. */
function entryFrames(
  entry: JsonRecord,
  prefix: string,
  stackOfPlates: boolean,
  isThreadSection: boolean,
  entryFirst: boolean,
  entryLast: boolean
): FrameState[] {
  const frames = asList(asRecord(entry["stacktrace"])?.["frames"]);
  const isException = entry["is_exception_stacktrace"] === true;
  const outermost = stackOfPlates ? entryFirst : entryLast;
  return frames.map((value, i) => {
    const frame = asRecord(value) ?? {};
    const first = i === 0;
    const last = i === frames.length - 1;
    let indicator: string | null = null;
    if (isException) {
      if (stackOfPlates ? first : last) {
        indicator = outermost ? `raise ${djStr(entry["type"])}` : `raise ${djStr(entry["type"])} (handled)`;
      } else if (stackOfPlates ? last : first) {
        indicator = outermost ? "→ begin" : "try…";
      }
    }
    return {
      key: `${prefix}-${i}`,
      frame,
      indicator,
      isRaisingFrame: !isThreadSection && (stackOfPlates ? entryFirst && first : entryLast && last),
      isFirstFrame: !isThreadSection && first && entryFirst
    };
  });
}

const BUTTON =
  "font-bold text-slate-500 dark:text-slate-300 border-slate-300 dark:border-slate-600 pl-4 pr-4 pb-1 pt-1 mr-2 border-2 rounded-md hover:bg-slate-200 dark:hover:bg-slate-800 active:ring-3";
const MENU_BUTTON =
  "block w-full text-left font-bold text-slate-500 dark:text-slate-300 pl-4 pr-4 pb-2 pt-2 hover:bg-slate-200 dark:hover:bg-slate-700";

type Props = {
  display: EventDisplay;
  platform: string;
  /** The event's title, for a stacktrace that isn't an exception's. */
  title: string;
  isLogMessage: boolean;
  /** "date (Event n of m total)" */
  eventLine: ReactNode;
  nav: ReactNode;
};

/** Upstream's `issues/stacktrace.html` with `issue_stacktrace.js`. */
export default function Stacktrace({ display, platform, title, isLogMessage, eventLine, nav }: Props) {
  const { stack_of_plates: stackOfPlates, multiple_thread_stacktraces: multipleThreads } = display.stacktrace;

  const entries: Entry[] = useMemo(() => {
    const raw = display.stacktrace.entries;
    return raw.map((value, i) => {
      const entry = asRecord(value) ?? {};
      return {
        key: `e${i}`,
        entry,
        isException: entry["is_exception_stacktrace"] === true,
        frames: entryFrames(entry, `e${i}`, stackOfPlates, false, i === 0, i === raw.length - 1)
      };
    });
  }, [display, stackOfPlates]);

  const threads: Entry[] = useMemo(() => {
    const raw = display.stacktrace.thread_stacktraces;
    return raw.map((value, i) => {
      const entry = asRecord(value) ?? {};
      return {
        key: `t${i}`,
        entry,
        isException: false,
        frames: entryFrames(entry, `t${i}`, stackOfPlates, true, i === 0, i === raw.length - 1)
      };
    });
  }, [display, stackOfPlates]);

  const allFrames = useMemo(() => [...entries, ...threads].flatMap((entry) => entry.frames), [entries, threads]);

  const [expanded, setExpanded] = useState<Record<string, boolean>>(() =>
    Object.fromEntries(allFrames.map(({ key, frame }) => [key, truthy(frame["raise_point"])]))
  );
  const [jumpTo, setJumpTo] = useState<{ id: string } | null>(null);
  const showMenu = useRef<HTMLDetailsElement>(null);

  useEffect(() => {
    window.addEventListener("scroll", setBodyMinHeight);
    return () => {
      window.removeEventListener("scroll", setBodyMinHeight);
      document.documentElement.style.minHeight = "";
    };
  }, []);

  useEffect(() => {
    if (jumpTo) {
      // repeated ids: like the browser, jump to the first one
      document.getElementById(jumpTo.id)?.scrollIntoView();
    }
  }, [jumpTo]);

  const setAll = (open: (frame: FrameState) => boolean) =>
    setExpanded(Object.fromEntries(allFrames.map((frame) => [frame.key, open(frame)])));
  const actions: [string, () => void][] = [
    ["Show all", () => setAll(() => true)],
    [
      "Show in-app",
      () => {
        setAll(({ frame }) => truthy(frame["in_app"]));
        setJumpTo({ id: "in-app" });
      }
    ],
    [
      "Show raise",
      () => {
        setAll(({ isRaisingFrame }) => isRaisingFrame);
        setJumpTo({ id: "raise" });
      }
    ],
    ["Collapse all", () => setAll(() => false)]
  ];
  const toggle = (key: string) => setExpanded((previous) => ({ ...previous, [key]: !previous[key] }));

  if (!entries.length) {
    return (
      <>
        <div className="flex">
          <div className="overflow-hidden">
            <div className="italic">{eventLine}</div>
          </div>
          <div className="ml-auto flex-none">
            <div className="flex place-content-end">{nav}</div>
          </div>
        </div>
        <div className="mt-6 mb-6 italic">No stacktrace available for this event.</div>
        <ThreadsSection threads={threads} platform={platform} expanded={expanded} onToggle={toggle} />
      </>
    );
  }

  return (
    <>
      {entries.map(({ key, entry, isException, frames }, i) => (
        <div key={key}>
          {(isException || i === 0) && (
            <div className={`${i > 0 ? "mt-4" : ""}${isLogMessage ? " mb-4" : ""}`}>
              <div className="flex items-start flex-col-reverse lg:flex-row xl:flex-col-reverse 2xl:flex-row">
                <div className="min-w-0 overflow-hidden">
                  {i === 0 && <div className="italic text-ellipsis whitespace-nowrap overflow-hidden">{eventLine}</div>}
                  <h1 className="text-2xl font-bold text-ellipsis whitespace-nowrap overflow-hidden">
                    {isException ? djStr(entry["type"]) : title}
                  </h1>
                </div>

                {i === 0 && (
                  <div className="ml-auto flex flex-none place-content-end pb-4 lg:pb-0 xl:pb-4 2xl:pb-0">
                    <div className="hidden 3xl:flex place-content-end">
                      {actions.map(([label, action]) => (
                        <button key={label} type="button" className={BUTTON} onClick={action}>
                          {label}
                        </button>
                      ))}
                    </div>

                    <div className="3xl:hidden relative">
                      <details ref={showMenu} className="relative">
                        <summary className="list-none cursor-pointer font-bold text-slate-500 dark:text-slate-300 border-slate-300 dark:border-slate-600 pl-4 pr-4 pb-2 pt-2 mr-2 border-2 rounded-md hover:bg-slate-200 dark:hover:bg-slate-800 active:ring-3">
                          Show{" "}
                          <svg
                            xmlns="http://www.w3.org/2000/svg"
                            fill="full"
                            viewBox="0 0 24 24"
                            strokeWidth="1.5"
                            stroke="currentColor"
                            className="w-4 h-4 inline"
                          >
                            <path d="M6.5,8.5l6,7l6-7H6.5z" />
                          </svg>
                        </summary>

                        <div className="absolute right-2 z-10 mt-1 min-w-40 bg-white dark:bg-slate-800 border-2 border-slate-300 dark:border-slate-600 rounded-md shadow-lg">
                          {actions.map(([label, action]) => (
                            <button
                              key={label}
                              type="button"
                              className={MENU_BUTTON}
                              onClick={() => {
                                action();
                                if (showMenu.current) {
                                  showMenu.current.open = false;
                                }
                              }}
                            >
                              {label}
                            </button>
                          ))}
                        </div>
                      </details>
                    </div>

                    <div className="flex place-content-end">{nav}</div>
                  </div>
                )}
              </div>

              {isException && (
                <div className="text-lg mb-4 text-ellipsis whitespace-nowrap overflow-hidden">
                  {djStr(entry["value"])}
                </div>
              )}
            </div>
          )}

          {!isException && multipleThreads && (
            <div className={`${i > 0 ? "mt-6" : ""} mb-4`}>
              <h2 className="text-xl font-bold">{djStr(entry["thread_title"])}</h2>
              {truthy(entry["thread_description"]) && (
                <div className="text-slate-600 dark:text-slate-300">{djStr(entry["thread_description"])}</div>
              )}
            </div>
          )}

          {!isException && !truthy(asRecord(entry["stacktrace"])?.["frames"]) && (
            <div className="mb-4 italic">No stacktrace found.</div>
          )}

          <Frames frames={frames} platform={platform} expanded={expanded} onToggle={toggle} />

          {isException &&
            i < entries.length - 1 &&
            (stackOfPlates ? (
              <div className="italic pt-4">
                The above exception was caused by or intentially reraised during the handling of the following
                exception:
              </div>
            ) : (
              <div className="italic pt-4">
                During handling of the above exception another exception occurred or was intentionally reraised:
              </div>
            ))}
        </div>
      ))}

      <ThreadsSection threads={threads} platform={platform} expanded={expanded} onToggle={toggle} />
    </>
  );
}

type ThreadsProps = {
  threads: Entry[];
  platform: string;
  expanded: Record<string, boolean>;
  onToggle: (key: string) => void;
};

/** With exceptions: all threads' stack traces. */
function ThreadsSection({ threads, platform, expanded, onToggle }: ThreadsProps) {
  if (!threads.length) {
    return null;
  }
  return (
    <>
      <h2 className="text-2xl font-bold mt-8 mb-4">Threads</h2>
      {threads.map(({ key, entry, frames }, i) => (
        <div key={key}>
          <div className={`${i > 0 ? "mt-6" : ""} mb-4`}>
            <h3 className="text-xl font-bold">{djStr(entry["thread_title"])}</h3>
            {truthy(entry["thread_description"]) && (
              <div className="text-slate-600 dark:text-slate-300">{djStr(entry["thread_description"])}</div>
            )}
          </div>

          {!truthy(asRecord(entry["stacktrace"])?.["frames"]) && (
            <div className="mb-4 italic">No stacktrace found.</div>
          )}

          <Frames frames={frames} platform={platform} expanded={expanded} onToggle={onToggle} />
        </div>
      ))}
    </>
  );
}
