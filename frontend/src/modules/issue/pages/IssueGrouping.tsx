import { useReducer } from "react";
import { useParams } from "react-router";

import Load from "common/components/Load";
import { useLoad } from "common/hooks/useLoad";
import IssueLayout from "modules/issue/components/IssueLayout";
import { useLastEvent, useQ } from "modules/issue/hooks";
import { issueApi } from "services/issue";

/** `GROUPING_CHOICES` */
const MECHANISMS: Record<string, string> = {
  "none": "Mechanism-independent (explicit fingerprint)",
  "bugsink-v1": "Original, default until v2.4.0 (July 2026)",
  "bugsink-v2": "Value-normalized (latest)"
};

/** Upstream's `issues/grouping.html`. */
export default function IssueGrouping() {
  const { issueId = "" } = useParams();
  const q = useQ();
  const [version, bump] = useReducer((n: number) => n + 1, 0);
  const last = useLastEvent(issueId, q, version);
  const groupings = useLoad(() => issueApi.groupings({ params: { issueId } }), [issueId, version]);

  return (
    <IssueLayout
      issueId={issueId}
      tab="grouping"
      version={version}
      onChanged={bump}
      event={null}
      requestRepr={last.data?.display?.request_repr ?? ""}
    >
      {() => (
        <>
          <h1 className="text-2xl font-bold text-ellipsis whitespace-nowrap overflow-hidden">Issue Grouping:</h1>
          <div className="mt-2 text-slate-700 dark:text-slate-300">Automatic grouping keys attached to this issue.</div>
          <Load loaded={groupings}>
            {(groupers) => (
              <ol className="mt-4 space-y-4">
                {groupers.map((grouper, i) => (
                  <li
                    key={i}
                    className="border-t border-slate-200 dark:border-slate-700 pt-4 first:border-t-0 first:pt-0"
                  >
                    <div className="text-sm font-bold text-slate-500 dark:text-slate-400 mb-1">
                      {MECHANISMS[grouper.grouping_mechanism] ?? grouper.grouping_mechanism}
                    </div>
                    <div className="font-mono text-sm break-all leading-relaxed">{grouper.grouping_key}</div>
                  </li>
                ))}
              </ol>
            )}
          </Load>
        </>
      )}
    </IssueLayout>
  );
}
