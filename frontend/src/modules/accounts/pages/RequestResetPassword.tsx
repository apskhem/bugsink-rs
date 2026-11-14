import { useState, type FormEvent } from "react";
import { Link } from "react-router";

import { FormErrors, InputField } from "common/components/FormField";
import { routes } from "common/constants/routes";
import { useTitle } from "common/hooks/useTitle";
import { accountsApi } from "services/accounts";
import type { FieldErrors } from "services/http";

import AccountBox from "../components/AccountBox";
import { ResetPasswordEmailSent } from "../components/EmailSent";
import { submitErrors } from "../utils/errors";

/** Upstream's `users/request_reset_password.html`: an email with a link to set a new password. */
export default function RequestResetPassword() {
  const [email, setEmail] = useState("");
  const [errors, setErrors] = useState<FieldErrors>({});
  const [sent, setSent] = useState(false);
  // sic: upstream's title for this page
  useTitle(sent ? null : "Resend confirmation");

  if (sent) {
    return <ResetPasswordEmailSent />;
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    try {
      await accountsApi.requestResetPassword({ email });
      setSent(true);
    } catch (error) {
      setErrors(submitErrors(error));
    }
  }

  return (
    <AccountBox>
      <form method="post" onSubmit={(event) => void submit(event)}>
        <FormErrors errors={errors["__all__"]} />
        <InputField
          label="Email"
          implicit
          errors={errors["email"]}
          type="email"
          name="email"
          id="id_email"
          maxLength={320}
          required
          value={email}
          onChange={(event) => setEmail(event.target.value)}
        />

        <button className="bg-slate-800 font-medium p-2 md:p-4 text-white uppercase w-full">Reset password</button>
      </form>

      <div className="mt-4">
        <Link to={routes.login()} className="text-slate-800 dark:text-slate-100">
          Log in instead
        </Link>
      </div>
    </AccountBox>
  );
}
