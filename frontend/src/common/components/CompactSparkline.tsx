import { formatEventCount } from "common/utils/format";

/**
 * Upstream's `events/_compact_event_sparkline.html`: the issue and project lists' 24 hourly counts (oldest first),
 * scaled per row with a floor of 10 so that a single event isn't a full-height spike.
 */
export default function CompactSparkline({ counts }: { counts: number[] }) {
  if (!counts.length) {
    return null;
  }
  const max = Math.max(10, ...counts);
  const title = `${formatEventCount(counts.reduce((sum, count) => sum + count, 0))} in the past 24h`;
  return (
    <div title={title} className="w-24">
      <div className="flex h-8 items-end gap-px">
        {counts.map((count, hour) => (
          <div key={hour} title={title} className="flex h-8 flex-1 items-end">
            {count ? (
              <div className="w-full bg-cyan-500" style={{ height: `${(count / max) * 100}%` }} />
            ) : (
              <div className="h-0.5 w-full bg-slate-300 dark:bg-slate-500" />
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
