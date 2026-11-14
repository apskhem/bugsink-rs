/**
 * How upstream's templates show event data (arbitrary JSON, parsed by Python): Python's truthiness, `str()` and
 * `repr()`, and what a Django template renders for a value.
 */

export type JsonRecord = Record<string, unknown>;

export function isRecord(value: unknown): value is JsonRecord {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** The object, or null for anything else. */
export function asRecord(value: unknown): JsonRecord | null {
  return isRecord(value) ? value : null;
}

/** The list, or an empty one for anything else. */
export function asList(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

/** Python's `bool(value)`. */
export function truthy(value: unknown): boolean {
  if (value === null || value === undefined || value === false || value === 0 || value === "") {
    return false;
  }
  if (Array.isArray(value)) {
    return value.length > 0;
  }
  if (isRecord(value)) {
    return Object.keys(value).length > 0;
  }
  return true;
}

/** Python's `repr()` of a string: single quotes unless only double quotes avoid escaping. */
function reprString(s: string): string {
  const quote = s.includes("'") && !s.includes('"') ? '"' : "'";
  let out = quote;
  for (const char of s) {
    const code = char.codePointAt(0) ?? 0;
    if (char === "\\") {
      out += "\\\\";
    } else if (char === quote) {
      out += `\\${quote}`;
    } else if (char === "\n") {
      out += "\\n";
    } else if (char === "\r") {
      out += "\\r";
    } else if (char === "\t") {
      out += "\\t";
    } else if (code < 0x20 || code === 0x7f) {
      out += `\\x${code.toString(16).padStart(2, "0")}`;
    } else {
      out += char;
    }
  }
  return out + quote;
}

/** Python's `str()` of a number as JSON parsing gives it. */
function numberStr(n: number): string {
  if (Number.isFinite(n)) {
    return String(n);
  }
  return Number.isNaN(n) ? "nan" : n > 0 ? "inf" : "-inf";
}

/** Python's `repr()` of a parsed JSON value. */
export function pyRepr(value: unknown): string {
  if (value === null || value === undefined) {
    return "None";
  }
  if (typeof value === "boolean") {
    return value ? "True" : "False";
  }
  if (typeof value === "number") {
    return numberStr(value);
  }
  if (typeof value === "string") {
    return reprString(value);
  }
  if (Array.isArray(value)) {
    return `[${value.map(pyRepr).join(", ")}]`;
  }
  if (isRecord(value)) {
    return `{${Object.entries(value)
      .map(([k, v]) => `${reprString(k)}: ${pyRepr(v)}`)
      .join(", ")}}`;
  }
  return String(value);
}

/** Python's `str()`: strings as they are, everything else as `repr()`. */
export function pyStr(value: unknown): string {
  return typeof value === "string" ? value : pyRepr(value);
}

/** What `{{ value }}` renders: a missing key renders nothing, None renders "None". */
export function djStr(value: unknown): string {
  return value === undefined ? "" : pyStr(value);
}
