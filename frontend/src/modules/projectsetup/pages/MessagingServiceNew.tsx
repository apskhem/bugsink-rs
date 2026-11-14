import { useState, type FormEvent } from "react";
import { Link, useNavigate } from "react-router";

import { NotFound } from "common/components/ErrorPage";
import { FormErrors, InputField, SelectField } from "common/components/FormField";
import Load from "common/components/Load";
import { routes } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { flash } from "common/hooks/useMessages";
import { useTitle } from "common/hooks/useTitle";
import { useSetNavProject } from "common/layouts/NavProject";
import ConfigFields from "modules/projectsetup/components/ConfigFields";
import { useIntParam } from "modules/projectsetup/hooks/useIntParam";
import { DISPLAY_NAME_HELP, saveButton } from "modules/projectsetup/utils/messagingServices";
import { formErrors, type FieldErrors } from "services/http";
import { projectsApi } from "services/projects";
import { projectSetupApi, type ProjectName, type ServiceKind } from "services/projectsetup";

/** The model's default kind. */
const DEFAULT_KIND = "slack";

function NewServiceForm({ project, kinds }: { project: ProjectName; kinds: ServiceKind[] }) {
  const navigate = useNavigate();
  const [displayName, setDisplayName] = useState("");
  const [kind, setKind] = useState(DEFAULT_KIND);
  // each kind's config form keeps its own values, as upstream's (hidden, disabled) blocks do
  const [configs, setConfigs] = useState<Record<string, Record<string, string>>>({});
  const [errors, setErrors] = useState<FieldErrors>({});

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    try {
      await projectSetupApi.addMessagingService(
        { display_name: displayName, kind, config: configs[kind] ?? {} },
        { params: { projectId: project.id } }
      );
      await navigate(routes.projectAlerts(project.id), flash("Messaging service added successfully."));
    } catch (error) {
      setErrors(formErrors(error));
    }
  }

  return (
    <form onSubmit={(event) => void submit(event)}>
      <div>
        <h1 className="text-4xl my-4 font-bold">Messaging Service · {project.name}</h1>
      </div>

      <FormErrors errors={errors["__all__"]} />

      <InputField
        label="Display name"
        errors={errors["display_name"]}
        helpText={DISPLAY_NAME_HELP}
        type="text"
        name="display_name"
        id="id_display_name"
        maxLength={100}
        required
        value={displayName}
        onChange={(event) => setDisplayName(event.target.value)}
      />
      <SelectField
        label="Kind"
        errors={errors["kind"]}
        name="kind"
        id="id_kind"
        required
        value={kind}
        onChange={(event) => setKind(event.target.value)}
      >
        {kinds.map((option) => (
          <option key={option.kind} value={option.kind}>
            {option.label}
          </option>
        ))}
      </SelectField>

      <div className="config-form-block block" data-config-kind={kind}>
        <ConfigFields
          kind={kind}
          values={configs[kind] ?? {}}
          errors={errors}
          onChange={(name, value) =>
            setConfigs((previous) => ({ ...previous, [kind]: { ...previous[kind], [name]: value } }))
          }
        />
      </div>

      <button name="action" value="add" className={saveButton}>
        Save
      </button>
      <Link to={routes.projectAlerts(project.id)} className="font-bold text-slate-500 dark:text-slate-300 ml-4">
        Cancel
      </Link>
    </form>
  );
}

function MessagingServiceNewPage({ projectId }: { projectId: number }) {
  const loaded = useLoad(
    () =>
      Promise.all([
        projectsApi.projectSummary({ params: { projectId } }),
        projectSetupApi.messagingServices({ params: { projectId } })
      ]),
    [projectId]
  );
  const project = loaded.data?.[0];
  useSetNavProject(project);
  useTitle(["Messaging Service", project?.name]);

  return (
    <Load loaded={loaded}>
      {([project, { kinds }]) => (
        <div className="flex items-center justify-center">
          <div className="m-4 max-w-4xl flex-auto">
            <NewServiceForm project={project} kinds={kinds} />
          </div>
        </div>
      )}
    </Load>
  );
}

/** `project_messaging_service_add` */
export default function MessagingServiceNew() {
  const projectId = useIntParam("projectId");
  return projectId === null ? <NotFound /> : <MessagingServiceNewPage key={projectId} projectId={projectId} />;
}
