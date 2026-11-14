/** Python's `f"{n:,}"`: thousands separated by commas. */
export function formatNumber(n: number): string {
  return n.toLocaleString("en-US");
}

/** Upstream's `_format_event_count`: "1 event", "1,234 events". */
export function formatEventCount(count: number): string {
  return `${formatNumber(count)} ${count === 1 ? "event" : "events"}`;
}
