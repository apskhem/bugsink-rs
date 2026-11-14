/**
 * Django's date formats as upstream's templates use them, in the browser's time zone (upstream: the installation's
 * `TIME_ZONE`, which this port doesn't have).
 */

export const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

const pad = (n: number, length = 2) => String(n).padStart(length, "0");

/** Django's `T`: the time zone's abbreviation. */
function timeZoneName(date: Date): string {
  const part = new Intl.DateTimeFormat("en-GB", { timeZoneName: "short" })
    .formatToParts(date)
    .find((p) => p.type === "timeZoneName");
  return part?.value ?? "";
}

/** `|date:"j M"` */
export function dayMonth(date: Date): string {
  return `${date.getDate()} ${MONTHS[date.getMonth()] ?? ""}`;
}

/** `|date:"j M G:i T"` */
export function dateTime(value: string | Date): string {
  const date = typeof value === "string" ? new Date(value) : value;
  return `${dayMonth(date)} ${date.getHours()}:${pad(date.getMinutes())} ${timeZoneName(date)}`;
}

/** `|date:"j M G:i:s"` */
export function dateTimeSeconds(date: Date): string {
  return `${dayMonth(date)} ${date.getHours()}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

/** `|date:"u"|slice:":3"`: the milliseconds. */
export function millis(date: Date): string {
  return pad(date.getMilliseconds(), 3);
}

/**
 * `compat.timestamp.parse_timestamp`: an ISO 8601 string or seconds since the epoch (number or numeric string); null
 * when it isn't either.
 */
export function parseTimestamp(value: unknown): Date | null {
  if (typeof value === "number") {
    return Number.isFinite(value) ? new Date(value * 1000) : null;
  }
  if (typeof value !== "string" || value.trim() === "") {
    return null;
  }
  const trimmed = value.trim();
  if (/^-?\d+(\.\d+)?$/.test(trimmed)) {
    return new Date(Number(trimmed) * 1000);
  }
  // naive timestamps are UTC
  const withZone = /([zZ]|[+-]\d\d:?\d\d)$/.test(trimmed) ? trimmed : `${trimmed}Z`;
  const date = new Date(withZone);
  return Number.isNaN(date.getTime()) ? null : date;
}
