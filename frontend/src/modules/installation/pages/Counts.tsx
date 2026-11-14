import Load from "common/components/Load";
import { useLoad } from "common/hooks/useLoad";
import { useTitle } from "common/hooks/useTitle";
import { formatNumber } from "common/utils/format";
import { installationApi } from "services/installation";

function capfirst(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/** `counts` (superusers): the number of rows per model, per app. */
export default function Counts() {
  useTitle("Model Counts");
  const loaded = useLoad(() => installationApi.counts(), []);
  return (
    <Load loaded={loaded}>
      {(counts) => (
        <div className="m-4">
          <div>
            <h1 className="text-4xl my-4 font-bold">Model Counts</h1>
          </div>

          {Object.entries(counts).map(([appName, modelCounts]) => {
            const models = Object.entries(modelCounts);
            return (
              <div key={appName}>
                <h1 className="text-2xl font-bold mt-4">{capfirst(appName)}</h1>

                <div className="mb-6">
                  {models.map(([model, count], index) => {
                    const first = index === 0;
                    const dotted =
                      index === models.length - 1
                        ? ""
                        : " border-b-2 border-dotted border-slate-300 dark:border-slate-600";
                    return (
                      <div
                        key={model}
                        className={`flex ${first ? "border-slate-300 dark:border-slate-600 border-t-2" : ""}`}
                      >
                        <div className={`w-1/6${dotted}`}>{model}</div>
                        <div className={`w-1/6${dotted} text-right`}>{formatNumber(count)}</div>
                        {/* upstream's "cached <time>": the counts here are never cached */}
                        <div className={`w-1/6${dotted} pl-4 text-slate-500 dark:text-slate-300`}>&nbsp;</div>
                        <div className={`w-1/2${dotted}`}>&nbsp;</div>
                      </div>
                    );
                  })}
                </div>
              </div>
            );
          })}
        </div>
      )}
    </Load>
  );
}
