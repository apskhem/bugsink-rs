import { useState } from "react";
import { Link, useNavigate, useParams } from "react-router";

import { LoadError } from "common/components/ErrorPage";
import { FormErrors } from "common/components/FormField";
import Load from "common/components/Load";
import { routes } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { useTitle } from "common/hooks/useTitle";
import TeamFields from "modules/teams/components/TeamFields";
import { primaryButton } from "modules/teams/components/buttons";
import { useFailure } from "modules/teams/hooks/useFailure";
import type { FieldErrors } from "services/http";
import { teamsApi, type Team, type TeamForm } from "services/teams";

/**
 * Upstream's `team_edit.html`: name and visibility, then on to the team's members. (Upstream's view can delete the
 * team too, but its page offers no button for that.)
 */
export default function TeamEdit() {
  const { teamId = "" } = useParams();
  const loaded = useLoad(() => teamsApi.team({ params: { teamId } }), [teamId]);
  useTitle(loaded.data ? ["Settings", loaded.data.name] : null);
  return <Load loaded={loaded}>{(team) => <EditForm team={team} />}</Load>;
}

function EditForm({ team }: { team: Team }) {
  const navigate = useNavigate();
  const { failure, fieldErrors } = useFailure();
  const [form, setForm] = useState<TeamForm>({ name: team.name, visibility: team.visibility });
  const [errors, setErrors] = useState<FieldErrors>({});

  async function save() {
    try {
      await teamsApi.editTeam(form, { params: { teamId: team.id } });
    } catch (error) {
      setErrors(fieldErrors(error));
      return;
    }
    await navigate(routes.teamMembers(team.id));
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
            <h1 className="text-4xl my-4 font-bold">Settings · {team.name}</h1>
          </div>

          <div className="mt-4 mb-4">Team settings for "{team.name}".</div>

          <FormErrors errors={errors["__all__"]} />
          <TeamFields value={form} onChange={setForm} errors={errors} />

          <div className="flex items-center mt-4">
            <button type="submit" className={primaryButton}>
              Save
            </button>
            <Link to={routes.teams()} className="text-cyan-500 dark:text-cyan-300 font-bold ml-2">
              Cancel
            </Link>
          </div>
        </form>
      </div>
    </div>
  );
}
