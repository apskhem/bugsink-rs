import { useReducer } from "react";
import { Link, useParams } from "react-router";

import Load from "common/components/Load";
import { useLoad } from "common/hooks/useLoad";
import IssueLayout from "modules/issue/components/IssueLayout";
import { useLastEvent, useQ } from "modules/issue/hooks";
import { tagDrilldownHref } from "modules/issue/utils/tags";
import { issueApi } from "services/issue";

const DOTTED = "border-b-2 border-dotted border-slate-300 dark:border-slate-600";

/** Upstream's `issues/tags.html`: per key, its 25 most frequent values. */
export default function IssueTags() {
  const { issueId = "" } = useParams();
  const q = useQ();
  const [version, bump] = useReducer((n: number) => n + 1, 0);
  const last = useLastEvent(issueId, q, version);
  const tags = useLoad(() => issueApi.tags({ params: { issueId }, queries: { all: true } }), [issueId, version]);

  return (
    <IssueLayout
      issueId={issueId}
      tab="tags"
      version={version}
      onChanged={bump}
      event={null}
      requestRepr={last.data?.display?.request_repr ?? ""}
    >
      {(issue) => (
        <Load loaded={tags}>
          {(keys) => (
            <>
              {keys.length === 0 && (
                <>
                  <h1 className="text-2xl font-bold mt-4">No tags</h1>
                  <div className="mb-6">No tags found for this issue.</div>
                </>
              )}
              {keys.map((keyTags) => {
                const values = [...keyTags.values, ...(keyTags.other ? [keyTags.other] : [])];
                return (
                  <div key={keyTags.key}>
                    <h1 id={keyTags.key} className="text-2xl font-bold mt-4">
                      {keyTags.key}:
                    </h1>
                    <div className="mb-6">
                      {values.map((tag, i) => {
                        const isLast = i === values.length - 1;
                        return (
                          <div
                            key={i}
                            className={`flex ${i === 0 ? "border-slate-300 dark:border-slate-600 border-t-2" : ""}`}
                          >
                            <div className={`w-2/3 flex items-center ${!isLast ? DOTTED : ""}`}>
                              {tag.value === null ? (
                                "Other..."
                              ) : (
                                <Link
                                  to={tagDrilldownHref(issue.project_id, q, keyTags.key, tag.value)}
                                  className="break-all hover:text-cyan-500 dark:hover:text-cyan-300"
                                  title="Filter issues by this tag"
                                >
                                  {tag.value}
                                </Link>
                              )}
                            </div>
                            <div className={`w-1/6 pr-4 flex items-stretch ${!isLast ? DOTTED : ""}`}>
                              <div className="bg-slate-200 dark:bg-slate-600" style={{ width: `${tag.pct}%` }}>
                                <span className="absolute pl-1 whitespace-nowrap">{tag.pct}%</span>
                              </div>
                            </div>
                            <div className={`w-1/6 ${!isLast ? ` ${DOTTED}` : ""} font-mono`}>{tag.count} events</div>
                          </div>
                        );
                      })}
                    </div>
                  </div>
                );
              })}
              {issue.digested_event_count > 25 && (
                <div className="mt-4 italic">
                  Note: Tags which have values that are mostly unique per event (such as{" "}
                  <span className="not-italic">trace, user, browser version and os version</span>) are not displayed
                  here.
                </div>
              )}
            </>
          )}
        </Load>
      )}
    </IssueLayout>
  );
}
