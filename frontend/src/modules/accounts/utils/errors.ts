import { apiError, formErrors, type FieldErrors } from "services/http";

/** Django's `SetPasswordMixin` / `UserCreationForm` message when the confirmation differs. */
export const PASSWORD_MISMATCH = "The two password fields didn’t match.";

/** A form's errors from a failed submit: the 400's by field, anything else (e.g. a 403) as an error of the form. */
export function submitErrors(error: unknown): FieldErrors {
  const failed = apiError(error);
  if (!failed) {
    throw error;
  }
  return failed.status === 400 ? formErrors(error) : { __all__: [failed.detail] };
}
