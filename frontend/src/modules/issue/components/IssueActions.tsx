import { TriangleIcon } from "modules/issue/components/Icons";
import { shortVersion } from "modules/issue/utils/releases";
import type { Issue } from "services/issue";

type MuteOption = { forOrUntil: "for" | "until"; periodName: string; nrOfPeriods: number; gteThreshold: number | null };

/** `GLOBAL_MUTE_OPTIONS` */
const MUTE_OPTIONS: MuteOption[] = [
  { forOrUntil: "for", periodName: "day", nrOfPeriods: 1, gteThreshold: null },
  { forOrUntil: "for", periodName: "week", nrOfPeriods: 1, gteThreshold: null },
  { forOrUntil: "for", periodName: "month", nrOfPeriods: 1, gteThreshold: null },
  { forOrUntil: "for", periodName: "month", nrOfPeriods: 3, gteThreshold: null },
  { forOrUntil: "until", periodName: "hour", nrOfPeriods: 1, gteThreshold: 5 },
  { forOrUntil: "until", periodName: "hour", nrOfPeriods: 24, gteThreshold: 5 },
  { forOrUntil: "until", periodName: "hour", nrOfPeriods: 24, gteThreshold: 100 }
];

function muteAction({ forOrUntil, periodName, nrOfPeriods, gteThreshold }: MuteOption): string {
  return `mute_${forOrUntil}:${periodName},${nrOfPeriods},${gteThreshold ?? "None"}`;
}

function muteLabel({ forOrUntil, periodName, nrOfPeriods, gteThreshold }: MuteOption): string {
  const plural = nrOfPeriods !== 1 ? "s" : "";
  if (forOrUntil === "for") {
    return `${nrOfPeriods} ${periodName}${plural}`;
  }
  return `${gteThreshold ?? ""} events per ${nrOfPeriods !== 1 ? `${nrOfPeriods} ${periodName}s` : periodName}`;
}

const PRIMARY =
  "font-bold text-slate-800 dark:text-slate-100 border-slate-500 dark:border-slate-400 pl-4 pr-4 pb-2 pt-2 ml-1 border-2 bg-cyan-200 dark:bg-cyan-700 hover:bg-cyan-400 dark:hover:bg-cyan-600 active:ring-3";
const MENU_ITEM =
  "block self-stretch font-bold text-slate-500 dark:text-slate-300 border-slate-300 dark:border-slate-600 pl-4 pr-4 pb-2 pt-2 border-l-2 border-r-2 border-b-2 bg-white dark:bg-slate-900 hover:bg-slate-200 dark:hover:bg-slate-800 active:ring-3 text-left whitespace-nowrap";

/** The buttons at the top of `issues/base.html`: resolve (in a release), mute (for/until), unmute, reopen. */
export default function IssueActions({ issue, onAction }: { issue: Issue; onAction: (action: string) => void }) {
  const button = (action: string, className: string, label: string, disabled = false) => (
    <button type="button" className={className} disabled={disabled} onClick={() => onAction(action)}>
      {label}
    </button>
  );
  const canMute = !issue.is_muted && !issue.is_resolved;

  return (
    <form onSubmit={(event) => event.preventDefault()}>
      {issue.is_resolved ? (
        button("reopen", `${PRIMARY} rounded-md`, "Reopen")
      ) : issue.project_has_releases ? (
        <>
          {button("resolve", `${PRIMARY} rounded-s-md`, "Resolve")}
          <div className="dropdown">
            {/* disabled b/c clicking the dropdown does nothing - we have hover for that */}
            <button
              type="button"
              disabled
              className="font-bold text-slate-800 dark:text-slate-100 fill-slate-800 dark:fill-slate-100 border-slate-500 dark:border-slate-400 pl-4 pr-4 pb-2 pt-2 border-r-2 border-t-2 border-b-2 hover:bg-slate-200 dark:hover:bg-slate-800 active:ring-3 rounded-e-md"
            >
              <TriangleIcon />
            </button>
            <div className="dropdown-content-right flex-col pl-2">
              {button("resolved_next", MENU_ITEM, "Resolved in next release")}
              {issue.latest_release !== null &&
                button(
                  `resolved_release:${issue.latest_release}`,
                  MENU_ITEM,
                  `Resolved in latest (${shortVersion(issue.latest_release)})`
                )}
            </div>
          </div>
        </>
      ) : (
        button("resolve", `${PRIMARY} rounded-md`, "Resolve")
      )}{" "}
      {canMute
        ? button(
            "mute",
            "font-bold text-slate-500 dark:text-slate-300 border-slate-300 dark:border-slate-600 pl-4 pr-4 pb-2 pt-2 ml-1 border-2 hover:bg-slate-200 dark:hover:bg-slate-800 active:ring-3 rounded-s-md",
            "Mute"
          )
        : button(
            "mute",
            "font-bold text-slate-300 dark:text-slate-600 border-slate-300 dark:border-slate-600 pl-4 pr-4 pb-2 pt-2 ml-1 border-2 rounded-s-md",
            "Mute",
            true
          )}
      <div className="dropdown">
        {canMute ? (
          <>
            <button
              type="button"
              disabled
              className="font-bold text-slate-500 dark:text-slate-300 fill-slate-500 dark:fill-slate-500 border-slate-300 dark:border-slate-600 pl-4 pr-4 pb-2 pt-2 border-r-2 border-b-2 border-t-2 hover:bg-slate-200 dark:hover:bg-slate-800 active:ring-3"
            >
              Mute for/until&nbsp;&nbsp;
              <TriangleIcon />
            </button>
            <div className="dropdown-content-right flex-col">
              {MUTE_OPTIONS.map((option) => (
                <button
                  key={muteAction(option)}
                  type="button"
                  className={MENU_ITEM}
                  onClick={() => onAction(muteAction(option))}
                >
                  {muteLabel(option)}
                </button>
              ))}
            </div>
          </>
        ) : (
          <button
            type="button"
            disabled
            className="font-bold text-slate-300 dark:text-slate-600 fill-slate-300 dark:fill-slate-600 border-slate-300 dark:border-slate-600 pl-4 pr-4 pb-2 pt-2 border-r-2 border-b-2 border-t-2"
          >
            Mute for/until&nbsp;&nbsp;
            <TriangleIcon />
          </button>
        )}
      </div>
      {issue.is_muted && !issue.is_resolved
        ? button(
            "unmute",
            "font-bold text-slate-500 dark:text-slate-300 border-slate-300 dark:border-slate-600 pl-4 pr-4 pb-2 pt-2 border-r-2 border-b-2 border-t-2 hover:bg-slate-200 dark:hover:bg-slate-800 active:ring-3 rounded-e-md",
            "Unmute"
          )
        : button(
            "unmute",
            "font-bold text-slate-300 dark:text-slate-600 border-slate-300 dark:border-slate-600 pl-4 pr-4 pb-2 pt-2 border-r-2 border-b-2 border-t-2 rounded-e-md",
            "Unmute",
            true
          )}
    </form>
  );
}
