import { useEffect } from "react";

import { useSession } from "contexts/session";

/**
 * Upstream's `{% block title %}`: the title's parts (undefined ones skipped), then the site title, joined by " · ";
 * `{ full }` is the whole title as is. null leaves the title alone (e.g. while there's nothing to show yet, or when
 * a component rendered instead sets its own).
 */
export function useTitle(title: string | readonly (string | undefined)[] | { full: string } | null) {
  const { settings } = useSession();
  const full =
    title === null
      ? null
      : typeof title === "object" && "full" in title
        ? title.full
        : [...(typeof title === "string" ? [title] : title), settings.site_title]
            .filter((part) => part !== undefined)
            .join(" · ");
  useEffect(() => {
    if (full !== null) {
      document.title = full;
    }
  }, [full]);
}
