/**
 * Upstream's `events/sparklines.py` (`get_issue_event_sparkline`): 28 days of hourly counts in buckets of 24, 12 and
 * 6 hours, aligned on local days (upstream: the installation's time zone; here: the browser's), with axis labels.
 */
import { formatEventCount, formatNumber } from "common/utils/format";
import { dayMonth } from "modules/issue/utils/dates";
import type { Counts, HourCount } from "services/issue";

const HOUR = 3600 * 1000;
const DAYS = 28;

export type SparklineBucket = {
  start: Date;
  end: Date;
  count: number;
  digestOrder: number | null;
  matchingCount: number | null;
  containsActiveEvent: boolean;
  pct: number;
  matchingPct: number | null;
  hasOverlay: boolean;
  clickCount: number;
  title: string;
};

export type SparklineVariant = {
  intervalHours: number;
  buckets: SparklineBucket[];
  xLabels: Date[];
  yLabels: number[];
};

/** Wall-clock arithmetic in local time, as Python's on aware datetimes. */
function addLocal(date: Date, { days = 0, hours = 0 }: { days?: number; hours?: number }): Date {
  return new Date(
    date.getFullYear(),
    date.getMonth(),
    date.getDate() + days,
    date.getHours() + hours,
    date.getMinutes(),
    date.getSeconds(),
    date.getMilliseconds()
  );
}

/** `get_sparkline_range`: [start, end) ending at the next `hourStep` boundary of the local day. */
function sparklineRange(now: Date, hourStep: number): { start: Date; end: Date } {
  const startOfDay = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const sinceStartOfDay = now.getTime() - startOfDay.getTime();
  const boundary = Math.ceil(sinceStartOfDay / (hourStep * HOUR)) * hourStep;
  const end = addLocal(startOfDay, { hours: boundary });
  return { start: addLocal(end, { days: -DAYS }), end };
}

function bucketEdges(start: Date, end: Date, hourStep: number): Date[] {
  const edges: Date[] = [];
  for (let curr = start; curr <= end; curr = addLocal(curr, { hours: hourStep })) {
    edges.push(curr);
  }
  return edges;
}

const hhmm = (d: Date) => `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
const sameDate = (a: Date, b: Date) =>
  a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();

/** `_format_bucket_label` */
function bucketLabel(start: Date, end: Date): string {
  if (start.getHours() === 0 && start.getMinutes() === 0 && addLocal(start, { days: 1 }).getTime() === end.getTime()) {
    return dayMonth(start);
  }
  if (sameDate(start, end)) {
    return `${dayMonth(start)} ${hhmm(start)} - ${hhmm(end)}`;
  }
  return `${dayMonth(start)} ${hhmm(start)} - ${dayMonth(end)} ${hhmm(end)}`;
}

/** `_get_bucket_title` */
function bucketTitle(label: string, count: number, matchingCount: number | null): string {
  if (matchingCount === null) {
    return `${label}: ${formatEventCount(count)}`;
  }
  const matching = `${formatNumber(matchingCount)} matching event${matchingCount === 1 ? "" : "s"}`;
  return `${label}: ${matching}, ${formatEventCount(count)} total`;
}

/** `get_x_labels` */
function xLabels(start: Date, end: Date, numLabels = 5): Date[] {
  const span = end.getTime() - start.getTime();
  return Array.from({ length: numLabels }, (_, i) => new Date(start.getTime() + (i * span) / (numLabels - 1)));
}

const NICE_STEPS = [1, 2, 2.5, 3, 4, 5, 6, 8, 10, 12, 12.5];
const NICE_STEP_RANK = new Map<number, number>([
  [1, 0],
  [2, 0.02],
  [2.5, 0.08],
  [5, 0.1],
  [10, 0.1],
  [3, 0.2],
  [4, 0.25],
  [6, 0.35],
  [8, 0.55],
  [12.5, 0.7],
  [12, 0.8]
]);

/** `math.isclose` with its default relative tolerance. */
const isClose = (a: number, b: number) => Math.abs(a - b) <= 1e-9 * Math.max(Math.abs(a), Math.abs(b));

function cleanLabel(value: number): number {
  const rounded = Math.round(value * 1e10) / 1e10;
  return isClose(rounded, Math.round(rounded)) ? Math.round(rounded) : rounded;
}

function niceBase(value: number): number {
  if (value === 0) {
    return 1;
  }
  const scaled = value / 10 ** Math.floor(Math.log10(Math.abs(value)));
  let best = NICE_STEPS[0] ?? 1;
  for (const candidate of NICE_STEPS) {
    if (Math.abs(candidate - scaled) < Math.abs(best - scaled)) {
      best = candidate;
    }
  }
  return best;
}

function scoreCandidate(labels: number[], stepBase: number, maxValue: number, maxLabels: number): number {
  const top = labels[0] ?? 0;
  const count = labels.length;
  let headroom = (top - maxValue) / Math.max(maxValue, 1);
  if (top - maxValue <= 1) {
    headroom = 0;
  }
  const missingLabels = (maxLabels - count) / Math.max(maxLabels - 1, 1);
  const isExact = isClose(top, maxValue) ? 1 : 0;
  return (
    headroom * 3.65 +
    missingLabels * 2.8 +
    (NICE_STEP_RANK.get(stepBase) ?? 0) * 2.35 +
    (NICE_STEP_RANK.get(niceBase(top)) ?? 0) * 1.1 -
    (count / maxLabels) * 1.25 -
    isExact * 0.2 +
    Math.max(0, headroom - 0.2) * 6.25 +
    Math.max(0, headroom - 0.5) * 10.5
  );
}

function yLabelCandidates(maxValue: number, maxLabels: number): [number[], number][] {
  const candidates: [number[], number][] = [];
  const seen = new Set<string>();
  const wantsIntegerLabels = Number.isInteger(maxValue);
  const rawStep = maxValue / (maxLabels - 1);
  const firstPower = Math.floor(Math.log10(rawStep)) - 2;
  const lastPower = Math.floor(Math.log10(maxValue)) + 2;
  for (let exponent = firstPower; exponent <= lastPower; exponent++) {
    const power = 10 ** exponent;
    for (const stepBase of NICE_STEPS) {
      const step = stepBase * power;
      const firstInterval = Math.ceil(maxValue / step - 1e-12);
      const lastInterval = maxLabels === 2 ? 1 : maxLabels - 1;
      for (let intervals = Math.max(1, firstInterval); intervals <= lastInterval; intervals++) {
        const labels: number[] = [];
        for (let i = intervals; i >= 0; i--) {
          labels.push(cleanLabel(i * step));
        }
        if (wantsIntegerLabels && labels.some((label) => !Number.isInteger(label))) {
          continue;
        }
        const key = labels.join(",");
        if (seen.has(key)) {
          continue;
        }
        seen.add(key);
        candidates.push([labels, stepBase]);
      }
    }
  }
  return candidates;
}

/** `get_y_labels`: "nice" labels from the top down to 0. */
export function yLabels(maxValue: number, maxLabels = 5): number[] {
  if (maxLabels <= 0) {
    return [];
  }
  if (maxLabels === 1) {
    return [cleanLabel(Math.max(1, maxValue))];
  }
  if (maxValue <= 1) {
    return [1, 0];
  }
  if (Number.isInteger(maxValue) && maxValue <= maxLabels - 1) {
    return Array.from({ length: maxValue + 1 }, (_, i) => maxValue - i);
  }
  const candidates = yLabelCandidates(maxValue, maxLabels);
  let best: [number[], number] | undefined;
  let bestScore = Infinity;
  for (const candidate of candidates) {
    const score =
      maxLabels === 2 ? (candidate[0][0] ?? 0) : scoreCandidate(candidate[0], candidate[1], maxValue, maxLabels);
    if (score < bestScore) {
      best = candidate;
      bestScore = score;
    }
  }
  return best?.[0] ?? [];
}

type ByHour = Map<number, HourCount>;

function byHour(hours: HourCount[]): ByHour {
  return new Map(hours.map((hour) => [new Date(hour.bucket).getTime(), hour]));
}

/** `_build_sized_bucket_series` */
function sizedBucketSeries(
  start: Date,
  end: Date,
  hourStep: number,
  hours: ByHour,
  matchingHours: ByHour | null,
  activeEventAt: Date | null
): SparklineVariant {
  const edges = bucketEdges(start, end, hourStep);
  const hasOverlay = matchingHours !== null;
  const raw: Omit<SparklineBucket, "pct" | "matchingPct" | "hasOverlay" | "clickCount" | "title">[] = [];
  for (let i = 1; i < edges.length; i++) {
    const bucketStart = edges[i - 1];
    const bucketEnd = edges[i];
    if (!bucketStart || !bucketEnd) {
      continue;
    }
    let count = 0;
    let digestOrder: number | null = null;
    let matchingCount = 0;
    let matchingDigestOrder: number | null = null;
    for (let t = bucketStart.getTime(); t < bucketEnd.getTime(); t += HOUR) {
      const hour = hours.get(t);
      if (hour) {
        count += hour.count;
        digestOrder = hour.digest_order;
      }
      const matching = matchingHours?.get(t);
      if (matching) {
        matchingCount += matching.count;
        if (
          matching.digest_order !== null &&
          (matchingDigestOrder === null || matching.digest_order > matchingDigestOrder)
        ) {
          matchingDigestOrder = matching.digest_order;
        }
      }
    }
    raw.push({
      start: bucketStart,
      end: bucketEnd,
      count,
      digestOrder: hasOverlay ? matchingDigestOrder : digestOrder,
      matchingCount: hasOverlay ? matchingCount : null,
      containsActiveEvent:
        activeEventAt !== null &&
        bucketStart.getTime() <= activeEventAt.getTime() &&
        activeEventAt.getTime() < bucketEnd.getTime()
    });
  }

  const maxValue = Math.max(10, ...raw.map((bucket) => bucket.count));
  const buckets = raw.map((bucket) => ({
    ...bucket,
    pct: (bucket.count / maxValue) * 100,
    hasOverlay,
    matchingPct: bucket.matchingCount === null ? null : (bucket.matchingCount / maxValue) * 100,
    clickCount: bucket.matchingCount ?? bucket.count,
    title: bucketTitle(bucketLabel(bucket.start, bucket.end), bucket.count, bucket.matchingCount)
  }));

  return { intervalHours: hourStep, buckets, xLabels: xLabels(start, end), yLabels: yLabels(maxValue, 4) };
}

/** `get_issue_event_sparkline`: the 24, 12 and 6 hour variants. */
export function issueSparkline(counts: Counts, now: Date, activeEventAt: Date | null): SparklineVariant[] {
  const hours = byHour(counts.hours);
  const matching = counts.matching === null ? null : byHour(counts.matching);
  return [24, 12, 6].map((hourStep) => {
    const { start, end } = sparklineRange(now, hourStep);
    return sizedBucketSeries(start, end, hourStep, hours, matching, activeEventAt);
  });
}
