/** Upstream's `issha`/`shortsha` filters, `summarized_releases` and `Release.get_short_version`. */

const SHA_LENGTHS = [12, 16, 20, 32, 40, 64];

/** Does this look like a sha? */
export function issha(value: string): boolean {
  return SHA_LENGTHS.includes(value.length) && /^[0-9a-f]+$/.test(value);
}

/** If this value looks like a version hash, make it short. */
export function shortsha(value: string): string {
  return issha(value) ? value.slice(0, 12) : value;
}

/** python-semver's `Version.parse`, after `is_valid_semver` strips a `package@` prefix. */
const SEMVER =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$/;

function isSemver(version: string): boolean {
  const at = version.lastIndexOf("@");
  return SEMVER.test(at === -1 ? version : version.slice(at + 1));
}

export function shortVersion(version: string): string {
  if (version === "") {
    return "«no version»";
  }
  return isSemver(version) ? version : version.slice(0, 12);
}

export type SummarizedRelease = { version: string; comma: boolean } | { hiddenCount: number; comma: boolean };

/** At most 7 versions; else the first 3, how many more, and the last 3. */
export function summarizedReleases(versions: string[]): SummarizedRelease[] {
  if (versions.length <= 7) {
    return versions.map((version, i) => ({ version, comma: i < versions.length - 1 }));
  }
  return [
    ...versions.slice(0, 3).map((version) => ({ version, comma: true })),
    { hiddenCount: versions.length - 6, comma: true },
    ...versions.slice(-3).map((version, i) => ({ version, comma: i < 2 }))
  ];
}
