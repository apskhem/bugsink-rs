import { muteOptions, shortVersion } from "modules/issues/utils";

import { DropdownTriangle } from "./icons";

const menuItem =
  "block self-stretch font-bold text-slate-500 dark:text-slate-300 border-slate-300 pl-4 pr-4 pb-2 pt-2 border-l-2 border-r-2 border-b-2 bg-white dark:bg-slate-900 hover:bg-slate-200 dark:hover:bg-slate-800 active:ring-3 text-left whitespace-nowrap";

type Props = {
  /** The project's latest release when it has releases (project lists only): offers "resolved in next/latest". */
  latestRelease: string | null;
  /** Upstream's static disabledness per tab (`disable_resolve_buttons` etc.). */
  disableResolve: boolean;
  disableMute: boolean;
  disableUnmute: boolean;
  onAction: (action: string) => void;
  onDelete: () => void;
};

/** The issue list's header buttons: resolve (with the release menu), mute (for/until), unmute, and "..." (delete). */
export default function ActionButtons({
  latestRelease,
  disableResolve,
  disableMute,
  disableUnmute,
  onAction,
  onDelete
}: Props) {
  const hasReleases = latestRelease !== null;
  return (
    <>
      {disableResolve ? (
        hasReleases ? (
          <>
            <button
              type="button"
              disabled
              className="font-bold text-slate-300 dark:text-slate-600 border-slate-300 dark:border-slate-600 pl-4 pr-4 pb-2 pt-2 ml-1 border-2 rounded-s-md"
            >
              Resolve
            </button>
            <button
              type="button"
              disabled
              className="font-bold text-slate-300 dark:text-slate-600 fill-slate-300 dark:fill-slate-600 border-slate-300 dark:border-slate-600 pl-4 pr-4 pb-2 pt-2 border-r-2 border-t-2 border-b-2 rounded-e-md"
            >
              <DropdownTriangle />
            </button>
          </>
        ) : (
          <button
            type="button"
            disabled
            className="font-bold text-slate-300 dark:text-slate-600 border-slate-300 dark:border-slate-600 pl-4 pr-4 pb-2 pt-2 ml-1 border-2 rounded-md"
          >
            Resolve
          </button>
        )
      ) : hasReleases ? (
        <>
          <button
            type="button"
            className="font-bold text-slate-800 dark:text-slate-100 border-slate-500 dark:border-slate-400 pl-4 pr-4 pb-2 pt-2 ml-1 border-2 bg-cyan-200 dark:bg-cyan-700 hover:bg-cyan-400 dark:hover:bg-cyan-600 active:ring-3 rounded-s-md"
            onClick={() => onAction("resolve")}
          >
            Resolve
          </button>
          <div className="dropdown">
            {/* disabled b/c clicking the dropdown does nothing - we have hover for that */}
            <button
              type="button"
              disabled
              className="font-bold text-slate-800 dark:text-slate-100 fill-slate-800 dark:fill-slate-100 border-slate-500 dark:border-slate-400 pl-4 pr-4 pb-2 pt-2 border-r-2 border-t-2 border-b-2 hover:bg-slate-200 dark:hover:bg-slate-800 active:ring-3 rounded-e-md"
            >
              <DropdownTriangle />
            </button>
            <div className="dropdown-content-right flex-col pl-2">
              <button
                type="button"
                className="block self-stretch font-bold text-slate-500 dark:text-slate-300 border-slate-300 dark:border-slate-600 pl-4 pr-4 pb-2 pt-2 border-l-2 border-r-2 border-b-2 bg-white dark:bg-slate-900 hover:bg-slate-200 dark:hover:bg-slate-800 active:ring-3 text-left whitespace-nowrap"
                onClick={() => onAction("resolved_next")}
              >
                Resolved in next release
              </button>
              <button
                type="button"
                className="block self-stretch font-bold text-slate-500 dark:text-slate-300 border-slate-300 dark:border-slate-600 pl-4 pr-4 pb-2 pt-2 border-l-2 border-r-2 border-b-2 bg-white dark:bg-slate-900 hover:bg-slate-200 dark:hover:bg-slate-800 active:ring-3 text-left whitespace-nowrap"
                onClick={() => onAction(`resolved_release:${latestRelease}`)}
              >
                Resolved in latest ({shortVersion(latestRelease)})
              </button>
            </div>
          </div>
        </>
      ) : (
        <button
          type="button"
          className="font-bold text-slate-800 dark:text-slate-100 border-slate-500 dark:border-slate-400 pl-4 pr-4 pb-2 pt-2 ml-1 border-2 bg-cyan-200 dark:bg-cyan-700 hover:bg-cyan-400 dark:hover:bg-cyan-600 active:ring-3 rounded-md"
          onClick={() => onAction("resolve")}
        >
          Resolve
        </button>
      )}{" "}
      {disableMute ? (
        <button
          type="button"
          disabled
          className="font-bold text-slate-300 dark:text-slate-600 border-slate-300 dark:border-slate-600 pl-4 pr-4 pb-2 pt-2 ml-1 border-2 rounded-s-md"
        >
          Mute
        </button>
      ) : (
        <button
          type="button"
          className="font-bold text-slate-500 dark:text-slate-300 border-slate-300 dark:border-slate-600 pl-4 pr-4 pb-2 pt-2 ml-1 border-2 hover:bg-slate-200 dark:hover:bg-slate-800 active:ring-3 rounded-s-md"
          onClick={() => onAction("mute")}
        >
          Mute
        </button>
      )}
      <div className="dropdown">
        {disableMute ? (
          <button
            type="button"
            disabled
            className="font-bold text-slate-300 dark:text-slate-600 fill-slate-300 dark:fill-slate-600 border-slate-300 dark:border-slate-600 pl-4 pr-4 pb-2 pt-2 border-r-2 border-b-2 border-t-2"
          >
            Mute for/until&nbsp;&nbsp;
            <DropdownTriangle />
          </button>
        ) : (
          <>
            <button
              type="button"
              disabled
              className="font-bold text-slate-500 dark:text-slate-300 fill-slate-500 dark:fill-slate-500 border-slate-300 dark:border-slate-600 pl-4 pr-4 pb-2 pt-2 border-r-2 border-b-2 border-t-2 hover:bg-slate-200 dark:hover:bg-slate-800 active:ring-3"
            >
              Mute for/until&nbsp;&nbsp;
              <DropdownTriangle />
            </button>
            <div className="dropdown-content-right flex-col">
              {muteOptions.map((option) => (
                <button key={option.action} type="button" className={menuItem} onClick={() => onAction(option.action)}>
                  {option.label}
                </button>
              ))}
            </div>
          </>
        )}
      </div>
      {disableUnmute ? (
        <button
          type="button"
          disabled
          className="font-bold text-slate-300 dark:text-slate-600 border-slate-300 dark:border-slate-600 pl-4 pr-4 pb-2 pt-2 border-r-2 border-b-2 border-t-2 rounded-e-md"
        >
          Unmute
        </button>
      ) : (
        <button
          type="button"
          className="font-bold text-slate-500 dark:text-slate-300 border-slate-300 dark:border-slate-600 pl-4 pr-4 pb-2 pt-2 border-r-2 border-b-2 border-t-2 hover:bg-slate-200 dark:hover:bg-slate-800 active:ring-3 rounded-e-md"
          onClick={() => onAction("unmute")}
        >
          Unmute
        </button>
      )}
      <div className="dropdown">
        <button
          type="button"
          disabled
          className="font-bold text-slate-500 fill-slate-500 border-slate-300 ml-2 pl-4 pr-4 pb-2 pt-2 border-2 hover:bg-slate-200 active:ring-3 rounded-md"
        >
          ...
        </button>
        <div className="dropdown-content-right flex-col">
          <button
            type="button"
            onClick={onDelete}
            className="block self-stretch font-bold text-red-500 dark:text-slate-300 border-slate-300 pl-4 pr-4 pb-2 pt-2 border-l-2 border-r-2 border-b-2 bg-white dark:bg-slate-900 hover:bg-red-50 dark:hover:bg-red-800 active:ring-3 text-left whitespace-nowrap"
          >
            Delete
          </button>
        </div>
      </div>
    </>
  );
}
