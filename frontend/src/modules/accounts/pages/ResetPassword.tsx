import { useState, type FormEvent } from "react";
import { Link, useNavigate, useParams } from "react-router";

import { LoadError } from "common/components/ErrorPage";
import { FormErrors, InputField } from "common/components/FormField";
import { routes } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { useTitle } from "common/hooks/useTitle";
import { useSession } from "contexts/session";
import { accountsApi } from "services/accounts";
import { isStatus, type FieldErrors } from "services/http";

import AccountBox from "../components/AccountBox";
import BareNotFound from "../components/BareNotFound";
import { goTo, useNext } from "../hooks/useNext";
import { PASSWORD_MISMATCH, submitErrors } from "../utils/errors";

/** Upstream's `Http404` for an unknown or expired token. */
const INVALID_TOKEN = "Invalid or expired token";

/**
 * Upstream's `users/reset_password.html` (also how invited and welcomed users set their first password): sets the
 * password, confirms the address, logs in and goes to `?next=` (when safe) or home.
 */
export default function ResetPassword() {
  const { token = "" } = useParams();
  const { setUser } = useSession();
  const navigate = useNavigate();
  const next = useNext();
  const checked = useLoad(() => accountsApi.checkResetPasswordToken({ params: { token } }), [token]);
  const [password1, setPassword1] = useState("");
  const [password2, setPassword2] = useState("");
  const [errors, setErrors] = useState<FieldErrors>({});
  const [invalid, setInvalid] = useState(false);

  const notFound = invalid || (checked.state === "failed" && isStatus(checked.error, 404));
  useTitle(notFound || checked.state !== "loaded" ? null : "Reset password");

  if (notFound) {
    return <BareNotFound message={INVALID_TOKEN} />;
  }
  if (checked.state === "failed") {
    return <LoadError error={checked.error} />;
  }
  if (checked.state === "loading") {
    return null;
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (password1 !== password2) {
      // checked here: the API takes the password once
      setErrors({ new_password2: [PASSWORD_MISMATCH] });
      return;
    }
    try {
      setUser(await accountsApi.resetPassword({ password: password1 }, { params: { token } }));
      goTo(navigate, next);
    } catch (error) {
      if (isStatus(error, 404)) {
        setInvalid(true);
        return;
      }
      const failed = submitErrors(error);
      setErrors({
        ...(failed["__all__"] && { __all__: failed["__all__"] }),
        // Django's `SetPasswordForm` puts the validators' errors on the confirmation
        ...(failed["password"] && { new_password2: failed["password"] })
      });
      // Django's PasswordInput doesn't render the values again
      setPassword1("");
      setPassword2("");
    }
  }

  return (
    <AccountBox>
      <form method="post" onSubmit={(event) => void submit(event)}>
        <FormErrors errors={errors["__all__"]} />
        <InputField
          label="New password"
          implicit
          errors={errors["new_password1"]}
          helpText="At least 8 characters"
          type="password"
          name="new_password1"
          id="id_new_password1"
          autoComplete="new-password"
          required
          value={password1}
          onChange={(event) => setPassword1(event.target.value)}
        />
        <InputField
          label="New password confirmation"
          implicit
          errors={errors["new_password2"]}
          type="password"
          name="new_password2"
          id="id_new_password2"
          autoComplete="new-password"
          required
          value={password2}
          onChange={(event) => setPassword2(event.target.value)}
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
