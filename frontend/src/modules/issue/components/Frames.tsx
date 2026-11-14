import { useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";

import { highlightLines } from "modules/issue/utils/highlight";
import { asList, asRecord, djStr, isRecord, pyRepr, pyStr, truthy, type JsonRecord } from "modules/issue/utils/python";

/** `issue_stacktrace.js`'s `setBodyMinHeight`: keeps the page from shrinking under the viewport while collapsing. */
export function setBodyMinHeight() {
  const html = document.documentElement;
  const distanceToWindowBottom = Math.max(html.offsetHeight - (window.scrollY + window.innerHeight), 0);
  html.style.minHeight = `${html.offsetHeight - distanceToWindowBottom}px`;
}

/** `collapseSection`/`expandSection`: animates the height between 0 and the content's. */
function Collapsible({ open, className, children }: { open: boolean; className: string; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  const [initialStyle] = useState(open ? undefined : { height: "0px" });
  const previous = useRef(open);

  useLayoutEffect(() => {
    const element = ref.current;
    if (!element || previous.current === open) {
      return undefined;
    }
    previous.current = open;
    setBodyMinHeight();
    const sectionHeight = `${element.scrollHeight}px`;
    if (open) {
      element.style.height = sectionHeight;
      const onTransitioned = () => {
        element.removeEventListener("transitionend", onTransitioned);
        if (element.style.height === sectionHeight) {
          element.style.removeProperty("height");
        }
      };
      element.addEventListener("transitionend", onTransitioned);
      return () => element.removeEventListener("transitionend", onTransitioned);
    }
    const transition = element.style.transition;
    element.style.transition = "";
    let inner = 0;
    const outer = requestAnimationFrame(() => {
      element.style.height = sectionHeight;
      element.style.transition = transition;
      inner = requestAnimationFrame(() => {
        element.style.height = "0px";
      });
    });
    return () => {
      cancelAnimationFrame(outer);
      cancelAnimationFrame(inner);
      element.style.height = "0px";
    };
  }, [open]);

  return (
    <div ref={ref} className={className} style={initialStyle}>
      {children}
    </div>
  );
}

/** JSON pointer segment (RFC 6901). */
const segment = (key: string) => key.replace(/~/g, "~0").replace(/\//g, "~1");

type Incomplete = JsonRecord;

function Trimmed({ count }: { count: unknown }) {
  return <i>&lt;{pyStr(count)} items trimmed…&gt;</i>;
}

/** Upstream's `format_var`: lists and dicts bracketed, keys as `repr()`, values as `str()`, trimmed items marked. */
export function FormatVar({ value, pointer, incomplete }: { value: unknown; pointer: string; incomplete: Incomplete }) {
  if (Array.isArray(value) || isRecord(value)) {
    const parts: ReactNode[] = Array.isArray(value)
      ? value.map((item, i) => <FormatVar key={i} value={item} pointer={`${pointer}/${i}`} incomplete={incomplete} />)
      : Object.entries(value).map(([k, v]) => (
          <span key={k}>
            {pyRepr(k)}: <FormatVar value={v} pointer={`${pointer}/${segment(k)}`} incomplete={incomplete} />
          </span>
        ));
    if (incomplete[pointer] !== undefined) {
      parts.push(<Trimmed key="trimmed" count={incomplete[pointer]} />);
    }
    const [open, close] = Array.isArray(value) ? ["[", "]"] : ["{", "}"];
    return (
      <>
        {open}
        {parts.map((part, i) => (
          <span key={i}>
            {i > 0 && ", "}
            {part}
          </span>
        ))}
        {close}
      </>
    );
  }
  return <>{pyStr(value)}</>;
}

/** The code listing: `pygmentize` and `firstlineno`. */
function CodeContext({ frame, platform }: { frame: JsonRecord; platform: string }) {
  const preContext = asList(frame["pre_context"]);
  const postContext = asList(frame["post_context"]);
  const lines = useMemo(
    () =>
      [...preContext, frame["context_line"], ...postContext].map((line) =>
        line === null || line === undefined ? "" : pyStr(line)
      ),
    [preContext, postContext, frame]
  );
  const filename = typeof frame["filename"] === "string" && frame["filename"] ? frame["filename"] : null;
  const highlighted = useMemo(() => highlightLines(lines, filename, platform), [lines, filename, platform]);
  const lineno = frame["lineno"];
  const firstLineno = typeof lineno === "number" ? lineno - preContext.length : null;

  const line = (i: number) => {
    const html = highlighted?.[i];
    return html === undefined ? (
      <div className="whitespace-pre w-full inline pr-6">{lines[i]} </div>
    ) : (
      <div className="whitespace-pre w-full inline pr-6" dangerouslySetInnerHTML={{ __html: `${html} ` }} />
    );
  };

  return (
    <div className="bg-slate-50 dark:bg-slate-800 syntax-coloring mt-6 mb-6">
      <ol
        className={`list-decimal overflow-x-auto list-inside pt-6 pb-6 ${firstLineno === null ? "list-none" : ""}`}
        start={firstLineno ?? undefined}
      >
        {preContext.map((_, i) => (
          <li key={i} className="pl-6">
            {line(i)}
          </li>
        ))}
        {/* the gradient is a workaround for a full-width line inside the overflow, when some other line overflows */}
        <li className="pl-6 bg-gradient-to-r from-slate-300 dark:from-slate-950 font-bold w-full">
          {line(preContext.length)}
        </li>
        {postContext.map((_, i) => (
          <li key={i} className="pl-6">
            {line(preContext.length + 1 + i)}
          </li>
        ))}
      </ol>
    </div>
  );
}

function Variables({ frame }: { frame: JsonRecord }) {
  const vars = frame["vars"];
  const incomplete = asRecord(frame["vars_incomplete"]) ?? {};
  const varsIncomplete = incomplete[""] !== undefined;
  const entries = isRecord(vars) ? Object.entries(vars) : [];
  const dotted = "border-b-2 border-dotted border-slate-300 dark:border-slate-600";
  return (
    <div className="mt-4 mb-6">
      <div className="flex">
        <div className="w-1/3 pt-2 font-bold border-b-2 border-slate-500 dark:border-slate-400 pl-4">Variable</div>
        <div className="w-2/3 pt-2 font-bold border-b-2 border-slate-500 dark:border-slate-400 pr-4">Value</div>
      </div>
      {entries.map(([name, value], i) => {
        const border = i < entries.length - 1 || varsIncomplete;
        return (
          <div key={name} className="flex">
            <div className={`w-1/3 pl-4 ${border ? dotted : ""}`}>{name}</div>
            <div className={`w-2/3 pr-4 ${border ? dotted : ""}`}>
              <FormatVar value={value} pointer={`/${segment(name)}`} incomplete={incomplete} />
            </div>
          </div>
        );
      })}
      {varsIncomplete && (
        <div className="flex">
          <div className="w-1/3 pl-4 italic">&lt;{pyStr(incomplete[""])} items trimmed…&gt;</div>
          <div className="w-2/3 pr-4"></div>
        </div>
      )}
    </div>
  );
}

export type FrameState = {
  key: string;
  frame: JsonRecord;
  /** "raise X", "→ begin", ... */
  indicator: string | null;
  isRaisingFrame: boolean;
  isFirstFrame: boolean;
};

type FramesProps = {
  frames: FrameState[];
  platform: string;
  expanded: Record<string, boolean>;
  onToggle: (key: string) => void;
};

/** Upstream's `issues/_stacktrace_frames.html`. */
export default function Frames({ frames, platform, expanded, onToggle }: FramesProps) {
  return frames.map(({ key, frame, indicator, isFirstFrame }, i) => {
    const open = expanded[key] ?? false;
    const inApp = truthy(frame["in_app"]);
    const hasContext = "context_line" in frame && frame["context_line"] !== null;
    const hasVars = truthy(frame["vars"]);
    const fn = frame["function"];
    const lineno = frame["lineno"];
    const colno = frame["colno"];
    const instructionAddr = frame["instruction_addr"];
    return (
      <div key={key} className="bg-white dark:bg-slate-700 w-full font-mono">
        {truthy(frame["raise_point"]) && <span id="raise"></span>}
        {inApp && <span id="in-app"></span>}
        {isFirstFrame && <span id="first-frame"></span>}

        <div
          className={`flex pl-4 pt-2 pb-2 border-b-2 ${i === 0 ? "border-t-2" : ""} bg-slate-100 dark:bg-slate-700 border-slate-400 cursor-pointer`}
          onClick={() => {
            // don't toggle when the user is selecting text (e.g. to copy a filename)
            if (!window.getSelection()?.toString()) {
              onToggle(key);
            }
          }}
        >
          <div className="text-ellipsis overflow-hidden">
            {inApp ? (
              <>
                <span className="font-bold">{djStr(frame["filename"])}</span>
                {truthy(fn) && (
                  <>
                    {" in "}
                    <span className="font-bold">{djStr(fn)}</span>
                  </>
                )}
                {truthy(lineno) && (
                  <>
                    {" line "}
                    <span className="font-bold">
                      {djStr(lineno)}
                      {truthy(colno) && `:${djStr(colno)}`}
                    </span>
                  </>
                )}
                {truthy(instructionAddr) && ` ${djStr(instructionAddr)}`}.
              </>
            ) : (
              <span className="italic">
                {djStr(frame["filename"])}
                {truthy(fn) && ` in ${djStr(fn)}`}
                {truthy(lineno) && ` line ${djStr(lineno)}${truthy(colno) ? `:${djStr(colno)}` : ""}`}
                {truthy(instructionAddr) && ` ${djStr(instructionAddr)}`}.
              </span>
            )}
          </div>

          <div className="ml-auto pr-4">
            {indicator && (
              <span className="bg-slate-200 dark:bg-slate-800 pl-2 pr-2 pt-1 pb-1 rounded-md whitespace-nowrap">
                {indicator}
              </span>
            )}
          </div>

          <div className="pr-4">
            <svg
              xmlns="http://www.w3.org/2000/svg"
              viewBox="0 0 20 20"
              fill="currentColor"
              className={`w-5 h-5 transition-all ${open ? "rotate-180" : ""}`}
            >
              <path
                fillRule="evenodd"
                d="M5.23 7.21a.75.75 0 011.06.02L10 11.168l3.71-3.938a.75.75 0 111.08 1.04l-4.25 4.5a.75.75 0 01-1.08 0l-4.25-4.5a.75.75 0 01.02-1.06z"
                clipRule="evenodd"
              />
            </svg>
          </div>
        </div>

        <Collapsible open={open} className="border-slate-400 overflow-hidden transition-all">
          <div className={`pl-6 pr-6 ${i < frames.length - 1 ? "border-b-2 border-slate-400" : ""}`}>
            {hasContext && <CodeContext frame={frame} platform={platform} />}
            {hasVars && <Variables frame={frame} />}
            {!hasContext && !hasVars && (
              <div className="mt-6 mb-6 italic">
                {truthy(frame["sourcemap_error"])
                  ? djStr(frame["sourcemap_error"])
                  : truthy(frame["debug_id"])
                    ? `No sourcemaps found for Debug ID ${djStr(frame["debug_id"])}`
                    : "No code context or variables available for this frame."}
              </div>
            )}
          </div>
        </Collapsible>
      </div>
    );
  });
}
