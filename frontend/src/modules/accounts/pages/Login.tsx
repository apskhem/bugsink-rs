import { useState, type FormEvent } from "react";
import { Link, useNavigate } from "react-router";

import { InputField } from "common/components/FormField";
import { routes } from "common/constants/routes";
import { useTitle } from "common/hooks/useTitle";
import { useSession } from "contexts/session";
import type { FieldErrors } from "services/http";
import { sessionApi } from "services/session";

import AccountBox from "../components/AccountBox";
import { goTo, useNext } from "../hooks/useNext";
import { submitErrors } from "../utils/errors";

/** Upstream's `bugsink/login.html` (Django's `LoginView`): logged in, it shows the form all the same. */
export default function Login() {
  useTitle("Log in");
  const { user, settings, setUser } = useSession();
  const navigate = useNavigate();
  const next = useNext();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [errors, setErrors] = useState<FieldErrors | null>(null);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    try {
      const me = await sessionApi.login({ username, password });
      setUser(me);
      goTo(navigate, next);
    } catch (error) {
      setErrors(submitErrors(error));
      setPassword(""); // Django's PasswordInput doesn't render the value again
    }
  }

  return (
    <AccountBox>
      {errors ? (
        errors["__all__"]?.map((error) => (
          <div key={error} className="mb-8 text-red-500 dark:text-red-400">
            {error}
          </div>
        ))
      ) : next ? (
        user ? (
          <div className="mb-8">
            Your account doesn't have access to this page. To proceed, please login with an account that has access.
          </div>
        ) : (
          <div className="mb-8">Please login to see this page.</div>
        )
      ) : null}

      <form method="post" onSubmit={(event) => void submit(event)}>
        <InputField
          label="Username"
          implicit
          errors={errors?.["username"]}
          type="text"
          name="username"
          id="id_username"
          autoFocus
          autoCapitalize="none"
          autoComplete="username"
          maxLength={150}
          required
          value={username}
          onChange={(event) => setUsername(event.target.value)}
        />
        <InputField
          label="Password"
          implicit
          errors={errors?.["password"]}
          type="password"
          name="password"
          id="id_password"
          autoComplete="current-password"
          required
          value={password}
          onChange={(event) => setPassword(event.target.value)}
        />

        <button className="bg-slate-800 font-medium p-2 md:p-4 text-white uppercase w-full">Login</button>
      </form>

      <div className="mt-4">
        <Link to={routes.requestResetPassword()} className="text-slate-800 dark:text-slate-100">
          Forgot password?
        </Link>
        {settings.signup && (
          <Link to={routes.signup()} className="float-right text-slate-800 dark:text-slate-100">
            Create an account
          </Link>
        )}
      </div>
    </AccountBox>
  );
}
