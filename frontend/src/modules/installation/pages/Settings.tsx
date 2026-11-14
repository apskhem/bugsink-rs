import Load from "common/components/Load";
import { useLoad } from "common/hooks/useLoad";
import { useTitle } from "common/hooks/useTitle";
import { installationApi } from "services/installation";

/** Upstream's `misc_settings` (Django's own settings), in its order; the rest are Bugsink's. */
const MISC_KEYS = [
  "ALLOWED_HOSTS",
  "SECURE_PROXY_SSL_HEADER",
  "USE_X_REAL_IP",
  "USE_X_FORWARDED_FOR",
  "X_FORWARDED_FOR_PROXY_COUNT",
  "TIME_ZONE",
  "EMAIL_HOST",
  "EMAIL_HOST_USER",
  "EMAIL_HOST_PASSWORD",
  "EMAIL_PORT",
  "EMAIL_USE_TLS",
  "EMAIL_USE_SSL",
  "EMAIL_BACKEND",
  "DEFAULT_FROM_EMAIL"
];

/** Python's `repr()` of a JSON value. */
function pyRepr(value: unknown): string {
  if (value === null || value === undefined) {
    return "None";
  }
  if (typeof value === "boolean") {
    return value ? "True" : "False";
  }
  if (typeof value === "string") {
    return `'${value.replace(/\\/g, "\\\\").replace(/'/g, "\\'")}'`;
  }
  if (Array.isArray(value)) {
    return `[${value.map(pyRepr).join(", ")}]`;
  }
  if (typeof value === "object") {
    return `{${Object.entries(value)
      .map(([k, v]) => `${pyRepr(k)}: ${pyRepr(v)}`)
      .join(", ")}}`;
  }
  return String(value);
}

/** `{{ value }}`: Python's `str()`. */
function pyStr(value: unknown): string {
  return typeof value === "string" ? value : pyRepr(value);
}

/** Django's `filesizeformat` (with its non-breaking space). */
function filesizeformat(bytes: number): string {
  const units = ["KB", "MB", "GB", "TB", "PB"];
  if (bytes < 1024) {
    return `${bytes} ${bytes === 1 ? "byte" : "bytes"}`;
  }
  let size = bytes / 1024;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }
  return `${size.toFixed(1)} ${units[unit]}`;
}

/** `round_values`: whole multiples of 1024 also as a file size. */
function maybeRound(value: unknown): string {
  if (typeof value === "number" && Number.isInteger(value) && value > 0 && value % 1024 === 0) {
    return `${value} (${filesizeformat(value)})`;
  }
  return pyStr(value);
}

function SettingsTable({ rows }: { rows: [string, string][] }) {
  return (
    <div className="mb-6">
      {rows.map(([key, value], index) => {
        const first = index === 0;
        const last = index === rows.length - 1;
        const dotted = last ? "" : "border-b-2 border-dotted border-slate-300 dark:border-slate-600";
        return (
          <div key={key} className={`flex ${first ? "border-slate-300 dark:border-slate-600 border-t-2" : ""}`}>
            <div className={`w-1/4 ${dotted}`}>{key}</div>
            <div className={`w-3/4 ${dotted} font-mono`}>{value}</div>
          </div>
        );
      })}
    </div>
  );
}

/** `settings_view` (superusers): the settings this installation runs with. */
export default function Settings() {
  useTitle("Settings");
  const loaded = useLoad(() => installationApi.settings(), []);
  return (
    <Load loaded={loaded}>
      {({ version, upstream_version, settings }) => {
        const entries = Object.entries(settings);
        const bugsink = entries.filter(([key]) => !MISC_KEYS.includes(key));
        const misc = MISC_KEYS.filter((key) => Object.hasOwn(settings, key));
        return (
          <div className="m-4">
            <div>
              <h1 className="text-4xl my-4 font-bold">Site Settings</h1>
            </div>

            <h1 className="text-2xl font-bold mt-4">Bugsink</h1>
            <SettingsTable rows={bugsink.map(([key, value]) => [key, maybeRound(value)])} />

            <h1 className="text-2xl font-bold mt-4">Misc</h1>
            <SettingsTable rows={misc.map((key) => [key, pyStr(settings[key])])} />

            <h1 className="text-2xl font-bold mt-4">System info</h1>

            <div className="mb-6">
              <div className="flex border-slate-300 dark:border-slate-600 border-t-2">
                <div className="w-1/4">Version</div>
                <div className="w-3/4">
                  {version} (Bugsink {upstream_version})
                </div>
              </div>
            </div>
          </div>
        );
      }}
    </Load>
  );
}
