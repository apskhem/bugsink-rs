import { useEffect, useState } from "react";
import { Link } from "react-router";

import { routes } from "common/constants/routes";
import { projectSetupApi } from "services/projectsetup";

const POLL_INTERVAL = 2000;

/**
 * Upstream's `includes/sdk_setup_event_status.html` with `sdk_setup_event_status.js`: polls the project's event count
 * and, once events arrive, slides in how many were received since the page was opened.
 */
export default function SdkSetupEventStatus({
  projectId,
  initialEventCount
}: {
  projectId: number;
  initialEventCount: number;
}) {
  const [received, setReceived] = useState(0);
  const [expanded, setExpanded] = useState(false);

  useEffect(() => {
    let timeout: ReturnType<typeof setTimeout>;
    let current = true;
    async function poll() {
      try {
        const { digested_event_count } = await projectSetupApi.sdkSetup({ params: { projectId } });
        if (current && digested_event_count - initialEventCount > 0) {
          setReceived(digested_event_count - initialEventCount);
        }
      } catch (error) {
        console.warn("Failed to fetch SDK setup status:", error);
      } finally {
        if (current) {
          timeout = setTimeout(() => void poll(), POLL_INTERVAL);
        }
      }
    }
    timeout = setTimeout(() => void poll(), POLL_INTERVAL);
    return () => {
      current = false;
      clearTimeout(timeout);
    };
  }, [projectId, initialEventCount]);

  const visible = received > 0;

  // made visible first, then expanded in the next frame, so that the transition runs
  useEffect(() => {
    if (!visible) {
      return undefined;
    }
    const frame = window.requestAnimationFrame(() => setExpanded(true));
    return () => window.cancelAnimationFrame(frame);
  }, [visible]);

  return (
    <div
      id="sdk-setup-event-status"
      className="grid transition-[grid-template-rows,opacity,margin-top] duration-300 ease-out motion-reduce:transition-none"
      style={{
        gridTemplateRows: expanded ? "1fr" : "0fr",
        opacity: expanded ? 1 : 0,
        marginTop: expanded ? "1rem" : 0,
        visibility: visible ? "visible" : "hidden"
      }}
      role="status"
      aria-live="polite"
      aria-atomic="true"
      aria-hidden={visible ? undefined : true}
    >
      <div className="min-h-0 overflow-hidden">
        <div className="bg-cyan-50 dark:bg-cyan-900 border-2 border-cyan-800 dark:border-cyan-400 p-4 rounded-lg">
          <span className="font-bold">
            {visible &&
              `${received.toLocaleString()} ${received === 1 ? "event" : "events"} received since opening this page.`}
          </span>{" "}
          <Link to={routes.issues(projectId)} className="font-bold underline">
            View the list of open issues.
          </Link>
        </div>
      </div>
    </div>
  );
}
