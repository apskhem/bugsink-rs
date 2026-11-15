import { useState, type FormEvent } from "react";
import { Link, useNavigate } from "react-router";

import { FormErrors, InputField } from "common/components/FormField";
import { routes } from "common/constants/routes";
import { useTitle } from "common/hooks/useTitle";
import { accountsApi } from "services/accounts";
import type { FieldErrors } from "services/http";

import { PASSWORD_MISMATCH, submitErrors } from "../utils/errors";

/** Django's `password_validators_help_text_html()` for upstream's `AUTH_PASSWORD_VALIDATORS`. */
const PASSWORD_HELP = (
  <ul>
    <li>Your password can’t be too similar to your other personal information.</li>
    <li>Your password must contain at least 8 characters.</li>
    <li>Your password can’t be a commonly used password.</li>
    <li>Your password can’t be entirely numeric.</li>
  </ul>
);

/** Upstream's `users/change_password.html` (Django's `PasswordChangeForm`); back to the preferences after. */
export default function ChangePassword() {
  useTitle("Change Password");
  const navigate = useNavigate();
  const [oldPassword, setOldPassword] = useState("");
  const [password1, setPassword1] = useState("");
  const [password2, setPassword2] = useState("");
  const [errors, setErrors] = useState<FieldErrors>({});

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (password1 !== password2) {
      // checked here: the API takes the new password once
      setErrors({ new_password2: [PASSWORD_MISMATCH] });
      return;
    }
    try {
      await accountsApi.changePassword({ old_password: oldPassword, new_password: password1 });
      await navigate(routes.preferences(), { state: { message: "Password updated" } });
    } catch (error) {
      const failed = submitErrors(error);
      setErrors({
        ...(failed["__all__"] && { __all__: failed["__all__"] }),
        ...(failed["old_password"] && { old_password: failed["old_password"] }),
        ...(failed["new_password"] && { new_password2: failed["new_password"] })
      });
      // Django's PasswordInput doesn't render the values again
      setOldPassword("");
      setPassword1("");
      setPassword2("");
    }
  }

  return (
    <div className="flex items-center justify-center">
      <div className="m-4 max-w-4xl flex-auto">
        <form method="post" onSubmit={(event) => void submit(event)}>
          <div>
            <h1 className="text-4xl my-4 font-bold">Change password</h1>
          </div>

          <FormErrors errors={errors["__all__"]} />
          <InputField
            label="Old password"
            errors={errors["old_password"]}
            type="password"
            name="old_password"
            id="id_old_password"
            autoComplete="current-password"
            autoFocus
            required
            value={oldPassword}
            onChange={(event) => setOldPassword(event.target.value)}
          />
          <InputField
            label="New password"
            errors={errors["new_password1"]}
            helpText={PASSWORD_HELP}
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
            errors={errors["new_password2"]}
            helpText="Enter the same password as before, for verification."
            type="password"
            name="new_password2"
            id="id_new_password2"
            autoComplete="new-password"
            required
            value={password2}
            onChange={(event) => setPassword2(event.target.value)}
          />

          <button className="font-bold text-slate-800 dark:text-slate-100 border-slate-500 dark:border-slate-400 pl-4 pr-4 pb-2 pt-2 border-2 bg-cyan-200 dark:bg-cyan-700 hover:bg-cyan-400 dark:hover:bg-cyan-600 active:ring-3 rounded-md">
            Change password
          </button>
          <Link to={routes.preferences()} className="text-cyan-500 dark:text-cyan-300 font-bold ml-2">
            Cancel
          </Link>
        </form>
      </div>
    </div>
  );
}
