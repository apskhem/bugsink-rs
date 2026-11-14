import type { ReactNode } from "react";

import type { Loaded } from "common/hooks/useLoad";

import { LoadError } from "./ErrorPage";

/** Renders `children` with the data once loaded; the error page if loading failed; nothing meanwhile. */
export default function Load<T>({ loaded, children }: { loaded: Loaded<T>; children: (data: T) => ReactNode }) {
  switch (loaded.state) {
    case "loading":
      return null;
    case "failed":
      return <LoadError error={loaded.error} />;
    case "loaded":
      return children(loaded.data);
  }
}
