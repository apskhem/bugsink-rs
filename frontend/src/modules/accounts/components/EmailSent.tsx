import { useTitle } from "common/hooks/useTitle";

import AccountBox from "./AccountBox";

/** Upstream's `users/confirm_email_sent.html` (after signup or asking for the confirmation again). */
export function ConfirmEmailSent() {
  useTitle("Verification email sent");
  return (
    <AccountBox>
      A verification email has been sent to your email address. Please verify your email address to complete the
      registration process.
    </AccountBox>
  );
}

/** Upstream's `users/reset_password_email_sent.html`. */
export function ResetPasswordEmailSent() {
  useTitle("Password reset sent");
  return (
    <AccountBox>
      A password reset link has been sent to your email address. Please check your inbox and follow the instructions to
      reset your password.
    </AccountBox>
  );
}
