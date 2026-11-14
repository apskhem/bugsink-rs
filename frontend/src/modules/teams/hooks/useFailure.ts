import { useCallback, useState } from "react";

import { formErrors, type FieldErrors } from "services/http";

/**
 * A failed action that upstream answers with an error page (403, 404, 500, ...): render `<LoadError>` for `failure`
 * instead of the page.
 */
export function useFailure() {
  const [failure, setFailure] = useState<{ error: unknown } | null>(null);
  const fail = useCallback((error: unknown) => setFailure({ error }), []);
  /** A form's field errors from a 400; anything else fails the page (and gives no field errors). */
  const fieldErrors = useCallback((error: unknown): FieldErrors => {
    try {
      return formErrors(error);
    } catch {
      setFailure({ error });
      return {};
    }
  }, []);
  return { failure, fail, fieldErrors };
}
