import { useState } from "react";
import { Link, useNavigate } from "react-router";

import { LoadError } from "common/components/ErrorPage";
import { FormErrors } from "common/components/FormField";
import { routes } from "common/constants/routes";
import { useTitle } from "common/hooks/useTitle";
import TeamFields from "modules/teams/components/TeamFields";
import { primaryButton } from "modules/teams/components/buttons";
import { useFailure } from "modules/teams/hooks/useFailure";
import type { FieldErrors } from "services/http";
import { teamsApi, type TeamForm } from "services/teams";

/** Upstream's `team_new.html`: on to the new team's members (you're its admin). */
export default function TeamNew() {
  useTitle(["New team"]);
  const navigate = useNavigate();
  const { failure, fieldErrors } = useFailure();
  const [form, setForm] = useState<TeamForm>({ name: "", visibility: 10 });
  const [errors, setErrors] = useState<FieldErrors>({});

  async function save() {
    let id;
    try {
      ({ id } = await teamsApi.createTeam(form));
    } catch (error) {
      setErrors(fieldErrors(error));
      return;
    }
    await navigate(routes.teamMembers(id));
  }

  if (failure) {
    return <LoadError error={failure.error} />;
  }

  return (
    <div className="flex items-center justify-center">
      <div className="m-4 max-w-4xl flex-auto">
        <form
          onSubmit={(event) => {
            event.preventDefault();
            void save();
          }}
        >
          <div>
            <h1 className="text-4xl my-4 font-bold">New Team</h1>
          </div>

          <FormErrors errors={errors["__all__"]} />
          <TeamFields value={form} onChange={setForm} errors={errors} />

          <button type="submit" className={primaryButton}>
            Save
          </button>
          <Link to={routes.teams()} className="text-cyan-500 dark:text-cyan-300 font-bold ml-2">
            Cancel
          </Link>
        </form>
      </div>
    </div>
  );
}
