import dayjs from "dayjs";
import { useState } from "react";
import { Link, useNavigate, useParams } from "react-router";

import { CopyButtonOnly } from "common/components/CopyButton";
import { LoadError } from "common/components/ErrorPage";
import { FormErrors, InputField } from "common/components/FormField";
import Load from "common/components/Load";
import { routes } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { useTitle } from "common/hooks/useTitle";
import { useSetNavProject } from "common/layouts/NavProject";
import {
  GroupingField,
  NameField,
  RetentionField,
  retentionValue,
  VisibilityField,
  type ProjectFormState
} from "modules/projects/components/ProjectFields";
import ConfirmModal from "modules/teams/components/ConfirmModal";
import { flash, success } from "modules/teams/components/Flash";
import { primaryButton } from "modules/teams/components/buttons";
import { useFailure } from "modules/teams/hooks/useFailure";
import type { FieldErrors } from "services/http";
import { projectsApi, type ProjectSettings } from "services/projects";

/** Upstream's `project_edit.html` (project admins only): the settings, the read-only slug and DSN, and deleting. */
export default function ProjectEdit() {
  const { projectId = "" } = useParams();
  const loaded = useLoad(() => projectsApi.projectSettings({ params: { projectId } }), [projectId]);
  useSetNavProject(loaded.data);
  useTitle(loaded.data ? ["Settings", loaded.data.name] : null);
  return <Load loaded={loaded}>{(project) => <EditForm key={project.id} project={project} />}</Load>;
}

function EditForm({ project }: { project: ProjectSettings }) {
  const navigate = useNavigate();
  const { failure, fail, fieldErrors } = useFailure();
  const [form, setForm] = useState<ProjectFormState>({
    name: project.name,
    visibility: project.visibility,
    retention: String(project.retention_max_event_count),
    grouping: project.grouping_mechanism
  });
  const [errors, setErrors] = useState<FieldErrors>({});
  const [deleting, setDeleting] = useState(false);
  const params = { params: { projectId: String(project.id) } };

  async function save() {
    try {
      await projectsApi.editProject(
        {
          name: form.name,
          visibility: form.visibility,
          retention_max_event_count: retentionValue(form.retention),
          grouping_mechanism: form.grouping
        },
        params
      );
    } catch (error) {
      setErrors(fieldErrors(error));
      return;
    }
    await navigate(routes.projects(), { state: flash([success("Project settings updated successfully.")]) });
  }

  async function remove() {
    try {
      await projectsApi.deleteProject(undefined, params);
    } catch (error) {
      fail(error);
      return;
    }
    await navigate(routes.projects(), {
      state: flash([success(`Project "${project.name}" has been deleted successfully.`)])
    });
  }

  if (failure) {
    return <LoadError error={failure.error} />;
  }

  const transition =
    project.previous_grouping_mechanism && project.grouping_transition_ends_at
      ? {
          previous: project.previous_grouping_mechanism,
          until: dayjs(project.grouping_transition_ends_at).format("YYYY-MM-DD")
        }
      : null;

  return (
    <>
      {deleting && (
        <ConfirmModal
          title="Delete Project"
          text="Are you sure you want to delete this project? This action cannot be undone and will delete all associated data."
          confirmLabel="Confirm"
          onCancel={() => setDeleting(false)}
          onConfirm={() => void remove()}
        />
      )}

      <div className="flex items-center justify-center">
        <div className="m-4 max-w-4xl flex-auto">
          <form
            onSubmit={(event) => {
              event.preventDefault();
              void save();
            }}
          >
            <div>
              <h1 className="text-4xl my-4 font-bold">Settings · {project.name}</h1>
            </div>

            <div className="mt-4 mb-4">Project settings for "{project.name}".</div>

            <FormErrors errors={errors["__all__"]} />

            <NameField value={form.name} onChange={(name) => setForm({ ...form, name })} errors={errors["name"]} />
            <InputField label="Slug (read-only)" type="text" name="slug" disabled value={project.slug} readOnly />
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

            <div>
              <div className="flex items-start gap-2">
                <div className="flex-1 min-w-0">
                  <InputField
                    label="DSN (read-only)"
                    type="text"
                    name="dsn"
                    disabled
                    value={project.dsn}
                    readOnly
                    helpText={
                      <>
                        Use the DSN to{" "}
                        <Link to={routes.projectSdkSetup(project.id)} className="text-cyan-800 font-bold">
                          set up the SDK
                        </Link>
                        .
                      </>
                    }
                  />
                </div>
                <div className="mt-8">
                  <CopyButtonOnly value={project.dsn} secondary />
                </div>
              </div>
            </div>
            <GroupingField
              value={form.grouping}
              onChange={(grouping) => setForm({ ...form, grouping })}
              errors={errors["grouping_mechanism"]}
              transition={transition}
            />

            <div className="flex items-center mt-4">
              <button type="submit" className={primaryButton}>
                Save
              </button>
              <Link to={routes.projects()} className="text-cyan-500 dark:text-cyan-300 font-bold ml-2">
                Cancel
              </Link>
              <button
                type="button"
                className="font-bold py-2 px-4 rounded-sm bg-red-500 dark:bg-red-700 text-white border-2 border-red-600 dark:border-red-400 hover:bg-red-600 dark:hover:bg-red-800 active:ring-3 ml-4 ml-auto"
                onClick={() => setDeleting(true)}
              >
                Delete Project
              </button>
            </div>
          </form>
        </div>
      </div>
    </>
  );
}
