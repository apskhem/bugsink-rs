import { useState, type FormEvent } from "react";
import { Link, useNavigate } from "react-router";

import { FormErrors, FormField, InputField, widgetClass } from "common/components/FormField";
import { routes } from "common/constants/routes";
import { useTitle } from "common/hooks/useTitle";
import { useSession } from "contexts/session";
import { accountsApi } from "services/accounts";
import type { FieldErrors } from "services/http";

import AccountBox from "../components/AccountBox";
import BareNotFound from "../components/BareNotFound";
import { ConfirmEmailSent } from "../components/EmailSent";
import { PASSWORD_MISMATCH, submitErrors } from "../utils/errors";

/** Upstream's `UserCreationForm.clean_username` message, which links to asking for the confirmation again. */
const NOT_CONFIRMED = "This email is already registered but not yet confirmed.";

/** Upstream's `signup.html`: the email address is the username; confirmed by email first, or logged in right away. */
export default function Signup() {
  const { settings, setUser } = useSession();
  const navigate = useNavigate();
  const [username, setUsername] = useState("");
  const [password1, setPassword1] = useState("");
  const [password2, setPassword2] = useState("");
  const [errors, setErrors] = useState<FieldErrors>({});
  const [sent, setSent] = useState(false);
  useTitle(settings.signup && !sent ? "Sign up" : null);

  if (!settings.signup) {
    return <BareNotFound message="User self-registration is not allowed." />;
  }
  if (sent) {
    return <ConfirmEmailSent />;
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (password1 !== password2) {
      // checked here: the API takes the password once (and would create the user)
      setErrors({ password2: [PASSWORD_MISMATCH] });
      return;
    }
    try {
      const result = await accountsApi.signup({ email: username, password: password1 });
      if ("confirmation_sent_to" in result) {
        setSent(true);
      } else {
        setUser(result);
        await navigate(routes.home());
      }
    } catch (error) {
      const failed = submitErrors(error);
      setErrors({
        ...(failed["__all__"] && { __all__: failed["__all__"] }),
        ...(failed["email"] && { username: failed["email"] }),
        ...(failed["password"] && { password1: failed["password"] })
      });
      // Django's PasswordInput doesn't render the values again
      setPassword1("");
      setPassword2("");
    }
  }

  const usernameErrors = errors["username"];

  return (
    <AccountBox>
      <form method="post" onSubmit={(event) => void submit(event)}>
        <FormErrors errors={errors["__all__"]} />

        <FormField label="Email" implicit>
          <input
            type="text"
            name="username"
            id="id_username"
            placeholder="Email"
            autoFocus
            autoCapitalize="none"
            autoComplete="username"
            maxLength={150}
            required
            value={username}
            onChange={(event) => setUsername(event.target.value)}
            className={widgetClass(usernameErrors)}
          />
          {usernameErrors?.map((error) => (
            <div key={error} className="text-red-500 dark:text-red-400 pt-1 px-2 text-sm">
              {error.startsWith(NOT_CONFIRMED) ? (
                <>
                  This email is already registered but not yet confirmed. Please check your email for the confirmation
                  link or{" "}
                  <b>
                    <Link to={`${routes.resendConfirmation()}?email=${encodeURIComponent(username)}`}>
                      request it again
                    </Link>
                  </b>
                  .
                </>
              ) : (
                error
              )}
            </div>
          ))}
        </FormField>
        <InputField
          label="Password"
          implicit
          errors={errors["password1"]}
          helpText="At least 8 characters"
          type="password"
          name="password1"
          id="id_password1"
          autoComplete="new-password"
          required
          value={password1}
          onChange={(event) => setPassword1(event.target.value)}
        />
        <InputField
          label="Password confirmation"
          implicit
          errors={errors["password2"]}
          type="password"
          name="password2"
          id="id_password2"
          autoComplete="new-password"
          required
          value={password2}
          onChange={(event) => setPassword2(event.target.value)}
        />

        <button className="bg-slate-800 font-medium p-2 md:p-4 text-white uppercase w-full">Sign up</button>
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
