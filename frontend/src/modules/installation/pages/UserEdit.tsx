import { useState, type FormEvent } from "react";
import { Link, useNavigate } from "react-router";

import { NotFound } from "common/components/ErrorPage";
import { FormErrors, InputField } from "common/components/FormField";
import Load from "common/components/Load";
import { routes } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { useTitle } from "common/hooks/useTitle";
import { useIntParam } from "modules/installation/hooks/useIntParam";
import { formErrors, type FieldErrors } from "services/http";
import { installationApi, type UserRow } from "services/installation";

function UserEditForm({ user }: { user: UserRow }) {
  useTitle(["Edit", user.username]);
  const navigate = useNavigate();
  const [email, setEmail] = useState(user.username);
  const [errors, setErrors] = useState<FieldErrors>({});

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    try {
      await installationApi.editUser({ email }, { params: { userId: user.id } });
      await navigate(routes.users());
    } catch (error) {
      setErrors(formErrors(error));
    }
  }

  return (
    <div className="flex items-center justify-center">
      <div className="m-4 max-w-4xl flex-auto">
        <form onSubmit={(event) => void submit(event)}>
          <div>
            <h1 className="text-4xl my-4 font-bold">Edit · {user.username}</h1>
          </div>

          <div className="mt-4 mb-4">Settings for {user.username}.</div>

          <FormErrors errors={errors["__all__"]} />

          <InputField
            label="Email"
            errors={errors["email"]}
            type="text"
            name="username"
            id="id_username"
            maxLength={150}
            required
            value={email}
            onChange={(event) => setEmail(event.target.value)}
          />

          <button
            name="action"
            value="invite"
            className="font-bold text-slate-800 dark:text-slate-100 border-slate-500 dark:border-slate-400 pl-4 pr-4 pb-2 pt-2 border-2 bg-cyan-200 dark:bg-cyan-700 hover:bg-cyan-400 dark:hover:bg-cyan-600 active:ring-3 rounded-md"
          >
            Save
          </button>
          <Link to={routes.users()} className="text-cyan-500 dark:text-cyan-300 font-bold ml-2">
            Cancel
          </Link>
        </form>
      </div>
    </div>
  );
}

function UserEditPage({ userId }: { userId: number }) {
  // there is no single-user endpoint: the user is looked up in the users admin's list
  const loaded = useLoad(() => installationApi.users(), []);
  return (
    <Load loaded={loaded}>
      {(users) => {
        const user = users.find((u) => u.id === userId);
        return user ? <UserEditForm user={user} /> : <NotFound />;
      }}
    </Load>
  );
}

/** `user_edit` (superusers): the user's email address, which is also their username. */
export default function UserEdit() {
  const userId = useIntParam("userId");
  return userId === null ? <NotFound /> : <UserEditPage key={userId} userId={userId} />;
}
