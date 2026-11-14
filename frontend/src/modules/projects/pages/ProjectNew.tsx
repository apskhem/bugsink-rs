import { useState } from "react";
import { Link, useNavigate } from "react-router";

import { LoadError } from "common/components/ErrorPage";
import { FormErrors, SelectField } from "common/components/FormField";
import Load from "common/components/Load";
import { routes } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { useTitle } from "common/hooks/useTitle";
import {
  GroupingField,
  NameField,
  RetentionField,
  retentionValue,
  VisibilityField,
  type ProjectFormState
} from "modules/projects/components/ProjectFields";
import { primaryButton } from "modules/teams/components/buttons";
import { useFailure } from "modules/teams/hooks/useFailure";
import type { FieldErrors } from "services/http";
import { projectsApi, type NewProjectOptions } from "services/projects";

/** Upstream's `project_new.html` (team admins only): on to the new project's SDK setup. */
export default function ProjectNew() {
  useTitle(["New Project"]);
  const loaded = useLoad(() => projectsApi.newProjectOptions(), []);
  return <Load loaded={loaded}>{(options) => <NewForm options={options} />}</Load>;
}

function NewForm({ options }: { options: NewProjectOptions }) {
  const navigate = useNavigate();
  const { failure, fieldErrors } = useFailure();
  // with just one team to choose from, upstream preselects it
  const [team, setTeam] = useState(options.teams.length === 1 ? (options.teams[0]?.id ?? "") : "");
  const [form, setForm] = useState<ProjectFormState>({
    name: "",
    visibility: 99,
    retention: String(options.retention_max_event_count),
    grouping: options.grouping_mechanism
  });
  const [errors, setErrors] = useState<FieldErrors>({});

  async function save() {
    let id;
    try {
      ({ id } = await projectsApi.createProject({
        ...(team ? { team_id: team } : {}),
        name: form.name,
        visibility: form.visibility,
        retention_max_event_count: retentionValue(form.retention),
        grouping_mechanism: form.grouping
      }));
    } catch (error) {
      setErrors(fieldErrors(error));
      return;
    }
    await navigate(routes.projectSdkSetup(id));
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
            <h1 className="text-4xl my-4 font-bold">New Project</h1>
          </div>

          <FormErrors errors={errors["__all__"]} />

          <SelectField
            label="Team"
            name="team"
            required
            value={team}
            onChange={(event) => setTeam(event.target.value)}
            errors={errors["team"]}
            helpText={
              options.teams.length ? undefined : (
                <>
                  You don't have any teams yet;{" "}
                  <Link to={routes.teamNew()} className="text-cyan-800 font-bold">
                    Create a team first.
                  </Link>
                </>
              )
            }
          >
            <option value="">---------</option>
            {options.teams.map((choice) => (
              <option key={choice.id} value={choice.id}>
                {choice.name}
              </option>
            ))}
          </SelectField>
          <NameField value={form.name} onChange={(name) => setForm({ ...form, name })} errors={errors["name"]} />
          <VisibilityField
            value={form.visibility}
            onChange={(visibility) => setForm({ ...form, visibility })}
            errors={errors["visibility"]}
          />
          <RetentionField
            value={form.retention}
            onChange={(retention) => setForm({ ...form, retention })}
            errors={errors["retention_max_event_count"]}
          />
          <GroupingField
            value={form.grouping}
            onChange={(grouping) => setForm({ ...form, grouping })}
            errors={errors["grouping_mechanism"]}
          />

          <button type="submit" className={primaryButton}>
            Save
          </button>
          <Link to={routes.projects()} className="text-cyan-500 dark:text-cyan-300 font-bold ml-2">
            Cancel
          </Link>
        </form>
      </div>
    </div>
  );
}
