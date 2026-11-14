import { useEffect, useState, type FormEvent } from "react";
import { Link, useLocation, useNavigate } from "react-router";

import { NotFound } from "common/components/ErrorPage";
import { FormErrors, InputField } from "common/components/FormField";
import Load from "common/components/Load";
import { routes } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { flash } from "common/hooks/useMessages";
import { useTitle } from "common/hooks/useTitle";
import { useSetNavProject } from "common/layouts/NavProject";
import ConfigFields from "modules/projectsetup/components/ConfigFields";
import WarningIcon from "modules/projectsetup/components/WarningIcon";
import { useIntParam } from "modules/projectsetup/hooks/useIntParam";
import { configValues, DISPLAY_NAME_HELP, formatDate, saveButton } from "modules/projectsetup/utils/messagingServices";
import { formErrors, type FieldErrors } from "services/http";
import { projectsApi } from "services/projects";
import { projectSetupApi, type MessagingService, type ProjectName } from "services/projectsetup";

/** `has_recent_failure`: how the most recent message failed. */
function FailureDetails({ service, failedAt }: { service: MessagingService; failedAt: string }) {
  const { hash } = useLocation();
  // the alerts page links here with `#failure-details`
  useEffect(() => {
    if (hash === "#failure-details") {
      document.getElementById("failure-details")?.scrollIntoView();
    }
  }, [hash]);

  return (
    <div
      id="failure-details"
      className="bg-red-50 dark:bg-red-900 border-2 border-red-200 dark:border-red-700 p-4 rounded-lg mb-6"
    >
      <h2 className="text-xl font-bold text-red-800 dark:text-red-200 mb-2 flex items-center">
        {/* nudged down by 1px to align visually with the text baseline (it's heavy in the bottom, light on top) */}
        <WarningIcon className="w-5 h-5 mr-2 relative top-[2px] align-middle" />
        Most Recent Message has Failed
      </h2>

      <div className="text-red-700 dark:text-red-300">
        <p>
          <strong>Last failure:</strong> {formatDate(failedAt, "MMMM D, YYYY, h:mm A")}
        </p>
        <p>
          <strong>Error type:</strong> {service.last_failure_error_type ?? "None"}
        </p>
        {!!service.last_failure_status_code && (
          <p>
            <strong>HTTP Status:</strong> {service.last_failure_status_code}
          </p>
        )}
        <p>
          <strong>Error message:</strong> {service.last_failure_error_message ?? "None"}
        </p>
        {service.last_failure_response_text && (
          <div className="mt-2">
            <p>
              <strong>Response{service.last_failure_is_json && " (JSON)"}:</strong>
            </p>
            <pre className="p-2 rounded-sm text-sm mt-1 overflow-x-auto">{service.last_failure_response_text}</pre>
          </div>
        )}
      </div>
    </div>
  );
}

function EditServiceForm({ project, service }: { project: ProjectName; service: MessagingService }) {
  const navigate = useNavigate();
  const [displayName, setDisplayName] = useState(service.display_name);
  const [config, setConfig] = useState(() => configValues(service.kind, service.config));
  const [errors, setErrors] = useState<FieldErrors>({});

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    try {
      await projectSetupApi.editMessagingService(
        { display_name: displayName, config },
        { params: { projectId: project.id, serviceId: service.id } }
      );
      await navigate(routes.projectAlerts(project.id), flash("Messaging service updated successfully."));
    } catch (error) {
      setErrors(formErrors(error));
    }
  }

  return (
    <form onSubmit={(event) => void submit(event)}>
      <div>
        <h1 className="text-4xl my-4 font-bold">Messaging Service · {project.name}</h1>
      </div>

      {service.last_failure_timestamp !== null && (
        <FailureDetails service={service} failedAt={service.last_failure_timestamp} />
      )}

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

      {/* the kind can't be changed */}
      <ConfigFields
        kind={service.kind}
        values={config}
        errors={errors}
        onChange={(name, value) => setConfig((previous) => ({ ...previous, [name]: value }))}
      />

      <button name="action" value="add" className={saveButton}>
        Save
      </button>
      <Link to={routes.projectAlerts(project.id)} className="font-bold text-slate-500 dark:text-slate-300 ml-4">
        Cancel
      </Link>
    </form>
  );
}

function MessagingServiceEditPage({ projectId, serviceId }: { projectId: number; serviceId: number }) {
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
      {([project, { services }]) => {
        const service = services.find((s) => s.id === serviceId);
        return service ? (
          <div className="flex items-center justify-center">
            <div className="m-4 max-w-4xl flex-auto">
              <EditServiceForm project={project} service={service} />
            </div>
          </div>
        ) : (
          <NotFound />
        );
      }}
    </Load>
  );
}

/** `project_messaging_service_edit`: its display name and config (not its kind). */
export default function MessagingServiceEdit() {
  const projectId = useIntParam("projectId");
  const serviceId = useIntParam("serviceId");
  if (projectId === null || serviceId === null) {
    return <NotFound />;
  }
  return <MessagingServiceEditPage key={`${projectId}/${serviceId}`} projectId={projectId} serviceId={serviceId} />;
}
