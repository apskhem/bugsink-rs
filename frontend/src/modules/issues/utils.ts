/** Upstream's `LOG_MESSAGE_TYPE`. */
const LOG_MESSAGE_TYPE = "Log Message";

/** `get_title_for_exception_type_and_value`: the type and the value's first line. */
export function issueTitle(type: string, value: string): string {
  if (!value) {
    return type;
  }
  const firstLine = value.split(/\r\n|[\n\r\v\f\x1c\x1d\x1e\x85\u2028\u2029]/)[0] ?? "";
  return type === LOG_MESSAGE_TYPE ? firstLine : `${type}: ${firstLine}`;
}

/** Django's `truncatechars`: at most `length` characters, the last one an ellipsis when cut. */
export function truncateChars(text: string, length: number): string {
  const chars = Array.from(text);
  return chars.length > length ? `${chars.slice(0, length - 1).join("")}…` : text;
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** Django's `date:"j M G:i T"` in the installation's time zone (UTC): "5 Nov 9:07 UTC". */
export function formatShortDate(iso: string): string {
  const d = new Date(iso);
  const minutes = String(d.getUTCMinutes()).padStart(2, "0");
  return `${d.getUTCDate()} ${MONTHS[d.getUTCMonth()] ?? ""} ${d.getUTCHours()}:${minutes} UTC`;
}

/** `Issue.friendly_id`: the project's slug in capitals, then the issue's number in the project. */
export function friendlyId(slug: string, digestOrder: number): string {
  return `${slug.toUpperCase()}-${digestOrder}`;
}

// semver.org's regex, as python-semver's `Version.parse`
const SEMVER =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$/;

/** `Release.get_short_version`: semver versions in full, others (e.g. hashes) cut to 12 characters. */
export function shortVersion(version: string): string {
  if (version === "") {
    return "«no version»";
  }
  // `RE_PACKAGE_VERSION`: an optional "package@" prefix
  const bare = version.slice(version.lastIndexOf("@") + 1);
  return SEMVER.test(bare) ? version : version.slice(0, 12);
}

/** `GLOBAL_MUTE_OPTIONS` with upstream's action values and labels. */
type MuteOption = { forOrUntil: "for" | "until"; periodName: string; nrOfPeriods: number; gteThreshold: number | null };

const MUTE_OPTIONS: MuteOption[] = [
  { forOrUntil: "for", periodName: "day", nrOfPeriods: 1, gteThreshold: null },
  { forOrUntil: "for", periodName: "week", nrOfPeriods: 1, gteThreshold: null },
  { forOrUntil: "for", periodName: "month", nrOfPeriods: 1, gteThreshold: null },
  { forOrUntil: "for", periodName: "month", nrOfPeriods: 3, gteThreshold: null },
  { forOrUntil: "until", periodName: "hour", nrOfPeriods: 1, gteThreshold: 5 },
  { forOrUntil: "until", periodName: "hour", nrOfPeriods: 24, gteThreshold: 5 },
  { forOrUntil: "until", periodName: "hour", nrOfPeriods: 24, gteThreshold: 100 }
];

export const muteOptions = MUTE_OPTIONS.map((option) => {
  const { forOrUntil, periodName, nrOfPeriods, gteThreshold } = option;
  const periods = nrOfPeriods === 1 ? periodName : `${nrOfPeriods} ${periodName}s`;
  return {
    action: `mute_${forOrUntil}:${periodName},${nrOfPeriods},${gteThreshold ?? "None"}`,
    label:
      forOrUntil === "for"
        ? `${nrOfPeriods} ${periodName}${nrOfPeriods === 1 ? "" : "s"}`
        : `${gteThreshold ?? ""} events per ${periods}`
  };
});
