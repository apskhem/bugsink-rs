import { useEffect, useState } from "react";
import { useLocation } from "react-router";

import { sessionApi, type SystemWarning } from "services/session";

/** Upstream's `system_warnings` banners, on every page with the navigation. */
export default function SystemWarnings() {
  const [warnings, setWarnings] = useState<SystemWarning[]>([]);
  const { pathname } = useLocation();

  useEffect(() => {
    let current = true;
    void sessionApi.systemWarnings().then(
      (loaded) => current && setWarnings(loaded),
      () => {}
    );
    return () => {
      current = false;
    };
  }, [pathname]);

  async function silence() {
    await sessionApi.silenceEmailWarning(undefined);
    setWarnings(await sessionApi.systemWarnings());
  }

  return warnings.map((warning) => (
    <div
      key={warning.message}
      className="flex p-4 bg-yellow-100 dark:bg-amber-800 border-b-2 border-yellow-200 dark:border-amber-900"
    >
      <div className="text-slate-800 dark:text-slate-100">
        {warning.message}
        {warning.link && (
          <>
            {" "}
            <a
              href={warning.link}
              target="_blank"
              rel="noopener noreferrer"
              className="font-bold text-slate-800 dark:text-slate-100"
            >
              More info
            </a>
          </>
        )}
      </div>
      {warning.silenceable && (
        <div className="ml-auto flex" title="Ignore">
          <button type="button" onClick={() => void silence()}>
            <svg
              xmlns="http://www.w3.org/2000/svg"
              fill="none"
              viewBox="0 0 24 24"
              strokeWidth="1.5"
              stroke="currentColor"
              className="size-6"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                d="m9.75 9.75 4.5 4.5m0-4.5-4.5 4.5M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z"
              />
            </svg>
          </button>
        </div>
      )}
    </div>
  ));
}
