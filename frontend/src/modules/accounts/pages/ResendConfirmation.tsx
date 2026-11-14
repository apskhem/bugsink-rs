import { useState, type FormEvent } from "react";
import { Link, useSearchParams } from "react-router";

import { FormErrors, InputField } from "common/components/FormField";
import { routes } from "common/constants/routes";
import { useTitle } from "common/hooks/useTitle";
import { accountsApi } from "services/accounts";
import { isStatus, type FieldErrors } from "services/http";

import AccountBox from "../components/AccountBox";
import BareNotFound from "../components/BareNotFound";
import { ConfirmEmailSent } from "../components/EmailSent";
import { submitErrors } from "../utils/errors";

/** Upstream's `users/resend_confirmation.html`; `?email=` fills in the address (signup links here with it). */
export default function ResendConfirmation() {
  const [searchParams] = useSearchParams();
  const [email, setEmail] = useState(() => searchParams.get("email") ?? "");
  const [errors, setErrors] = useState<FieldErrors>({});
  const [state, setState] = useState<"form" | "sent" | "not-found">("form");
  useTitle(state === "form" ? "Resend confirmation" : null);

  if (state === "sent") {
    return <ConfirmEmailSent />;
  }
  if (state === "not-found") {
    // the address is confirmed already (upstream: a 404 too), or unknown (upstream: a 500)
    return <BareNotFound />;
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    try {
      await accountsApi.resendConfirmation({ email });
      setState("sent");
    } catch (error) {
      if (isStatus(error, 404)) {
        setState("not-found");
      } else {
        setErrors(submitErrors(error));
      }
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

        <button className="bg-slate-800 font-medium p-2 md:p-4 text-white uppercase w-full">
          Resend verification email
        </button>
      </form>

      <div className="mt-4">
        <Link to={routes.requestResetPassword()} className="text-slate-800 dark:text-slate-100">
          Forgot password?
        </Link>
        <Link to={routes.login()} className="float-right text-slate-800 dark:text-slate-100">
          Log in instead
        </Link>
      </div>
    </AccountBox>
  );
}
