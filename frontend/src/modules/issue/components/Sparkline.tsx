import { Link, useLocation } from "react-router";

import { routes } from "common/constants/routes";
import { dayMonth } from "modules/issue/utils/dates";
import type { SparklineBucket, SparklineVariant } from "modules/issue/utils/sparkline";

/** `events/_event_sparkline_bucket.html` */
function Bucket({ bucket, clickable }: { bucket: SparklineBucket; clickable: boolean }) {
  return (
    <>
      {bucket.containsActiveEvent && (
        <div className="absolute top-0 bottom-0 left-1/2 z-10 w-[2px] -translate-x-1/2 bg-red-500 dark:bg-red-400"></div>
      )}
      {bucket.hasOverlay ? (
        <>
          <div
            className="absolute bottom-0 w-full bg-slate-300 dark:bg-slate-600"
            style={{ height: `${bucket.pct}%` }}
          ></div>
          {(clickable || !!bucket.matchingCount) && (
            <div
              className={`absolute bottom-0 w-full bg-slate-500 ${clickable ? "group-hover:bg-slate-600" : ""} dark:bg-slate-300 ${clickable ? "dark:group-hover:bg-slate-200" : ""}`}
              style={{ height: `${bucket.matchingPct ?? 0}%` }}
            ></div>
          )}
        </>
      ) : (
        <div
          className={`bg-slate-500 ${clickable ? "group-hover:bg-slate-600" : ""} dark:bg-slate-300 ${clickable ? "dark:group-hover:bg-slate-200" : ""} w-full`}
          style={{ height: `${bucket.pct}%` }}
        ></div>
      )}
    </>
  );
}

const VARIANT_CLASS: Record<number, string> = {
  24: "grid md:hidden",
  12: "hidden md:grid 2xl:hidden",
  6: "hidden 2xl:grid"
};

/**
 * Upstream's `events/_event_sparkline.html` for an issue (`event_sparkline_clickable`): a bucket with events opens its
 * last (matching) event on the details tab.
 */
export default function Sparkline({ issueId, variants }: { issueId: string; variants: SparklineVariant[] }) {
  const { search } = useLocation();
  return variants.map((variant) => (
    <div
      key={variant.intervalHours}
      className={`mt-8 ${VARIANT_CLASS[variant.intervalHours] ?? "grid"}`}
      style={{ gridTemplateColumns: "max-content 1fr", gridTemplateRows: "auto auto", columnGap: "0.5rem" }}
    >
      {/* y-axis: first and last are half-cells, so that the outermost labels align with the chart's edges */}
      <div className="flex flex-col justify-between h-24 mr-2 text-right tabular-nums text-xs text-slate-700 dark:text-slate-200">
        {variant.yLabels.map((label, i) =>
          i === 0 ? (
            <div key={i} className="flex-[0.5] flex items-start justify-end relative">
              <span className="-translate-y-[35%]">{label}</span>
            </div>
          ) : i === variant.yLabels.length - 1 ? (
            <div key={i} className="flex-[0.5] flex items-end justify-end relative">
              <span className="translate-y-[35%]">{label}</span>
            </div>
          ) : (
            <div key={i} className="flex-1 flex items-center justify-end">
              {label}
            </div>
          )
        )}
      </div>

      {/* chart area */}
      <div className="row-span-1">
        <div
          className="grid h-24"
          style={{ gridTemplateColumns: `repeat(${variant.buckets.length}, minmax(0,1fr))`, gap: "2px" }}
        >
          {variant.buckets.map((bucket) =>
            bucket.clickCount && bucket.digestOrder ? (
              <Link
                key={bucket.start.getTime()}
                className="group relative flex items-end cursor-pointer"
                to={`${routes.event(issueId, bucket.digestOrder, "details")}${search}`}
                title={bucket.title}
              >
                <Bucket bucket={bucket} clickable />
              </Link>
            ) : (
              <div key={bucket.start.getTime()} className="relative flex items-end" title={bucket.title}>
                <Bucket bucket={bucket} clickable={false} />
              </div>
            )
          )}
        </div>

        {/* baseline directly under bars */}
        <div className="h-[1px] border-b border-slate-300 dark:border-slate-500"></div>

        {/* x-axis */}
        <div className="mt-1 flex text-xs">
          {variant.xLabels.map((label, i) =>
            i === 0 ? (
              <div key={i} className="flex-[0.5] flex justify-start">
                <span>{dayMonth(label)}</span>
              </div>
            ) : i === variant.xLabels.length - 1 ? (
              <div key={i} className="flex-[0.5] flex justify-end">
                <span className="text-right">{dayMonth(label)}</span>
              </div>
            ) : (
              <div key={i} className="flex-1 flex justify-center">
                {dayMonth(label)}
              </div>
            )
          )}
        </div>
      </div>

      {/* bottom-left stays empty */}
      <div></div>
    </div>
  ));
}
