import { useEffect, useState } from "react";
import { useSearchParams } from "react-router";

import { useLoad } from "common/hooks/useLoad";
import { isStatus } from "services/http";
import { issueApi, type EventPage } from "services/issue";
import { projectsApi } from "services/projects";

/** The page's `?q=`. */
export function useQ(): string {
  const [searchParams] = useSearchParams();
  return searchParams.get("q") ?? "";
}

/** The project's name, for the navigation. */
export function useProjectName(projectId: number | undefined): string | null {
  const loaded = useLoad(
    () =>
      projectId === undefined
        ? Promise.resolve(null)
        : projectsApi.projectSummary({ params: { projectId } }).then((project) => project.name),
    [projectId]
  );
  return loaded.data ?? null;
}

/**
 * The issue's last event within the search (null when there is none): upstream's tabs that aren't about an event show
 * its request in the header; its navigation counts the matching events.
 */
export function useLastEvent(issueId: string, q: string, version: number) {
  return useLoad(async (): Promise<EventPage | null> => {
    try {
      return await issueApi.event({ params: { issueId, selector: "last" }, queries: q ? { q } : {} });
    } catch (error) {
      if (isStatus(error, 404)) {
        return null;
      }
      throw error;
    }
  }, [issueId, q, version]);
}

/** Upstream's `gravatar_sha`: SHA-256 of the trimmed, lower-cased address (empty where Web Crypto isn't available). */
export function useGravatarSha(email: string): string {
  const [sha, setSha] = useState("");
  useEffect(() => {
    let current = true;
    const subtle = globalThis.crypto?.subtle;
    if (!subtle) {
      return undefined;
    }
    void subtle.digest("SHA-256", new TextEncoder().encode(email.trim().toLowerCase())).then((digest) => {
      if (current) {
        setSha(
          Array.from(new Uint8Array(digest))
            .map((b) => b.toString(16).padStart(2, "0"))
            .join("")
        );
      }
    });
    return () => {
      current = false;
    };
  }, [email]);
  return sha;
}
