import { useState } from "react";
import { Link, Navigate, useNavigate, useParams } from "react-router";

import { LoadError } from "common/components/ErrorPage";
import { FormErrors, InputField } from "common/components/FormField";
import Load from "common/components/Load";
import { routes } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { useTitle } from "common/hooks/useTitle";
import { useSession } from "contexts/session";
import { useFailure } from "modules/teams/hooks/useFailure";
import type { FieldErrors } from "services/http";
import { membersApi, type GroupKind } from "services/members";

/**
 * Posting the token answers once with "no password needed" (and uses the token up), so the answer is kept: React's
 * development double effects would otherwise get a 404 the second time.
 */
const answers = new Map<string, Promise<{ needs_password: boolean }>>();

function acceptNewUser(kind: GroupKind, id: string, token: string) {
  const key = `${kind}/${id}/${token}`;
  let answer = answers.get(key);
  if (!answer) {
    answer = membersApi.acceptNewUser({ token }, { params: { kind, id } });
    answers.set(key, answer);
    answer.catch(() => answers.delete(key));
  }
  return answer;
}

type Props = {
  kind: GroupKind;
  /** The logged-in accept page (upstream's `<team|project>_members_accept`). */
  acceptUrl: (id: string) => string;
};

/**
 * Upstream's `<team|project>_members_accept_new_user` (the link in an invitation email to a new user, so no login
 * needed): with the token, a user without a password sets one first (upstream's `reset_password` page, which logs
 * them in), then on to the accept page.
 */
export default function AcceptNewUser({ kind, acceptUrl }: Props) {
  const { id = "", token = "" } = useParams();
  const loaded = useLoad(() => acceptNewUser(kind, id, token), [kind, id, token]);
  return (
    <Load loaded={loaded}>
      {({ needs_password }) =>
        needs_password ? <SetPassword token={token} next={acceptUrl(id)} /> : <Navigate to={acceptUrl(id)} replace />
      }
    </Load>
  );
}

/** Upstream's `users/reset_password.html` (its `SetPasswordForm`), continuing to `next`. */
function SetPassword({ token, next }: { token: string; next: string }) {
  useTitle(["Reset password"]);
  const { setUser } = useSession();
  const navigate = useNavigate();
  const { failure, fieldErrors } = useFailure();
  const [password1, setPassword1] = useState("");
  const [password2, setPassword2] = useState("");
  const [errors, setErrors] = useState<FieldErrors>({});

  async function submit() {
    if (password1 !== password2) {
      setErrors({ new_password2: ["The two password fields didn’t match."] });
      return;
    }
    let user;
    try {
      user = await membersApi.setPassword({ password: password1 }, { params: { token } });
    } catch (error) {
      const { password, ...rest } = fieldErrors(error);
      // the backend's one password field is upstream's confirmation field, where Django puts the validation errors
      setErrors(password ? { ...rest, new_password2: password } : rest);
      return;
    }
    setUser(user);
    await navigate(next, { replace: true });
  }

  if (failure) {
    return <LoadError error={failure.error} />;
  }

  return (
    <div className="bg-cyan-100 dark:bg-cyan-900 h-screen overflow-y-scroll flex items-center justify-center">
      <div className="bg-white dark:bg-slate-900 lg:w-5/12 md:6/12 w-10/12">
        <div className="bg-slate-200 dark:bg-slate-800 absolute left-1/2 transform -translate-x-1/2 -translate-y-1/2 rounded-full p-4 md:p-8">
          <Link to={routes.home()}>
            <img
              src="/images/bugsink-logo.png"
              className="h-8 w-8 md:h-16 md:w-16 dark:hidden block"
              alt="Bugsink logo"
            />
            <img
              src="/images/bugsink-logo-dark.png"
              className="h-8 w-8 md:h-16 md:w-16 hidden dark:block"
              alt="Bugsink logo"
            />
          </Link>
        </div>

        <div className="p-12 md:pt-24 md:pl-24 md:pr-24 md:pb-16">
          <form
            onSubmit={(event) => {
              event.preventDefault();
              void submit();
            }}
          >
            <FormErrors errors={errors["__all__"]} />

            <InputField
              label="New password"
              implicit
              type="password"
              name="new_password1"
              autoComplete="new-password"
              required
              value={password1}
              onChange={(event) => setPassword1(event.target.value)}
              errors={errors["new_password1"]}
              helpText="At least 8 characters"
            />
            <InputField
              label="New password confirmation"
              implicit
              type="password"
              name="new_password2"
              autoComplete="new-password"
              required
              value={password2}
              onChange={(event) => setPassword2(event.target.value)}
              errors={errors["new_password2"]}
            />

            <button type="submit" className="bg-slate-800 font-medium p-2 md:p-4 text-white uppercase w-full">
              Reset password
            </button>
          </form>

          <div className="mt-4">
            <Link to={routes.login()} className="text-slate-800 dark:text-slate-100">
              Log in instead
            </Link>
          </div>
        </div>
      </div>
    </div>
  );
}
