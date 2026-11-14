import { routes } from "common/constants/routes";

/** Upstream's `tag_search` template tag: a `key:"value"` clause, the key quoted when needed. */
function quoted(value: string): string {
  return `"${value.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
}

function tagClause(key: string, value: string): string {
  const k = /[\s:"\\]/.test(key) ? quoted(key) : key;
  return `${k}:${quoted(value)}`;
}

/** `tag_drilldown`: the project's issue list, searching the current search plus this tag. */
export function tagDrilldownHref(projectId: number, currentQ: string, key: string, value: string): string {
  const clause = tagClause(key, value);
  const q = currentQ ? `${currentQ} ${clause}`.trim() : clause;
  return `${routes.issues(projectId)}?q=${encodeURIComponent(q)}`;
}
