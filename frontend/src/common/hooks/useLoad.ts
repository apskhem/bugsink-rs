import { useCallback, useEffect, useState, type DependencyList } from "react";

export type Loaded<T> =
  | { state: "loading"; data?: undefined; error?: undefined; reload: () => void }
  | { state: "loaded"; data: T; error?: undefined; reload: () => void }
  | { state: "failed"; data?: undefined; error: unknown; reload: () => void };

/**
 * Runs `load` when `deps` change (and on `reload()`); a stale result (deps changed meanwhile) is dropped. While
 * reloading, the previous data stays.
 */
export function useLoad<T>(load: () => Promise<T>, deps: DependencyList): Loaded<T> {
  const [result, setResult] = useState<{ data: T } | { error: unknown } | null>(null);
  const [generation, setGeneration] = useState(0);
  const reload = useCallback(() => setGeneration((g) => g + 1), []);

  useEffect(() => {
    let current = true;
    load().then(
      (data) => current && setResult({ data }),
      (error: unknown) => current && setResult({ error })
    );
    return () => {
      current = false;
    };
    // the caller's deps
  }, [...deps, generation]);

  if (result === null) {
    return { state: "loading", reload };
  }
  return "data" in result
    ? { state: "loaded", data: result.data, reload }
    : { state: "failed", error: result.error, reload };
}
