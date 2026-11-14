import { useState, type FormEvent } from "react";
import { Link, useNavigate } from "react-router";

import { InputField, SelectField } from "common/components/FormField";
import Load from "common/components/Load";
import { routes } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { flash } from "common/hooks/useMessages";
import { useTitle } from "common/hooks/useTitle";
import { formErrors, type FieldErrors } from "services/http";
import { installationApi, type NewAuthToken } from "services/installation";

type CapabilityGroup = { prefix: string; capabilities: { capability: string; name: string; description: string }[] };

/** `AuthTokenForm.capability_groups`: the capabilities by prefix, in upstream's order. */
function capabilityGroups(capabilities: NewAuthToken["capabilities"]): CapabilityGroup[] {
  const groups: CapabilityGroup[] = [];
  for (const [capability, description] of capabilities) {
    const colon = capability.indexOf(":");
    const prefix = capability.slice(0, colon);
    const item = { capability, name: capability.slice(colon + 1), description };
    const group = groups.find((g) => g.prefix === prefix);
    if (group) {
      group.capabilities.push(item);
    } else {
      groups.push({ prefix, capabilities: [item] });
    }
  }
  return groups;
}

/** The model's field name for a capability (`CAPABILITY_FIELD_NAMES`). */
function fieldName(capability: string): string {
  return capability.replace(/[:-]/g, "_");
}

function AuthTokenForm({ options }: { options: NewAuthToken }) {
  const navigate = useNavigate();
  const superuser = options.can_choose_user_bound;
  const [description, setDescription] = useState("");
  // everyone else's tokens are always tied to their user account
  const [isUserBound, setIsUserBound] = useState(!superuser);
  const [projectId, setProjectId] = useState("");
  const [capabilities, setCapabilities] = useState<ReadonlySet<string>>(new Set());
  const [errors, setErrors] = useState<FieldErrors>({});

  function toggle(capability: string, checked: boolean) {
    const next = new Set(capabilities);
    if (checked) {
      next.add(capability);
    } else {
      next.delete(capability);
    }
    setCapabilities(next);
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    try {
      await installationApi.createAuthToken({
        description,
        is_user_bound: isUserBound,
        project_id: projectId ? Number(projectId) : null,
        // in upstream's order
        capabilities: options.capabilities.map(([capability]) => capability).filter((c) => capabilities.has(c))
      });
      await navigate(routes.authTokens(), flash("Token created"));
    } catch (error) {
      setErrors(formErrors(error));
    }
  }

  const nonFieldErrors = [...(errors["__all__"] ?? []), ...(errors["capabilities"] ?? [])];

  return (
    <form onSubmit={(event) => void submit(event)}>
      <h1 className="text-4xl my-4 font-bold">New Auth Token</h1>

      <InputField
        label="Description"
        errors={errors["description"]}
        type="text"
        name="description"
        id="id_description"
        maxLength={255}
        required
        value={description}
        onChange={(event) => setDescription(event.target.value)}
      />

      <div className="text-lg mb-6 md:mb-8">
        <label className="flex items-center gap-3 font-bold text-slate-800 dark:text-slate-100">
          <input
            type="checkbox"
            name="is_user_bound"
            id="id_is_user_bound"
            checked={isUserBound}
            disabled={!superuser}
            onChange={(event) => setIsUserBound(event.target.checked)}
          />
          Personal token
        </label>
        {errors["is_user_bound"]?.length ? (
          errors["is_user_bound"].map((error) => (
            <div key={error} className="text-red-500 dark:text-red-400 pt-1 px-2 text-sm">
              {error}
            </div>
          ))
        ) : (
          <div className="text-gray-500 dark:text-gray-400 pt-1 px-2 text-sm">
            {superuser
              ? "Tie this token to your own user account and current permissions."
              : "Your tokens are always tied to your user account."}
          </div>
        )}
      </div>

      <SelectField
        label="Project"
        errors={errors["project"]}
        helpText="Limit this token to one project, or leave it on the first option."
        name="project"
        id="id_project"
        value={projectId}
        onChange={(event) => setProjectId(event.target.value)}
      >
        <option value="">{superuser ? "All projects" : "All my projects"}</option>
        {options.projects.map(([id, name, teamName]) => (
          <option key={id} value={id}>
            {teamName === null ? name : `${teamName} / ${name}`}
          </option>
        ))}
      </SelectField>

      <fieldset className="mb-6 md:mb-8">
        <legend className="text-lg font-bold text-slate-800 dark:text-slate-100 mb-3">Capabilities</legend>
        {nonFieldErrors.map((error) => (
          <div key={error} className="mb-4 rounded-md bg-red-50 p-4 text-red-700 dark:bg-red-900 dark:text-red-300">
            {error}
          </div>
        ))}
        {capabilityGroups(options.capabilities).map((group) => (
          <div
            key={group.prefix}
            className="grid grid-cols-[7rem_minmax(0,1fr)] gap-x-4 border-t border-slate-200 dark:border-slate-700"
          >
            <div className="pt-4 text-right">
              <code className="font-bold text-slate-800 dark:text-slate-100 whitespace-nowrap">{group.prefix}:</code>
            </div>
            <div>
              {group.capabilities.map(({ capability, name, description: help }) => (
                <label
                  key={capability}
                  htmlFor={`id_${fieldName(capability)}`}
                  className="flex items-start gap-3 py-3 border-b border-slate-200 dark:border-slate-700"
                >
                  <span className="pt-1">
                    <input
                      type="checkbox"
                      name={fieldName(capability)}
                      id={`id_${fieldName(capability)}`}
                      checked={capabilities.has(capability)}
                      onChange={(event) => toggle(capability, event.target.checked)}
                    />
                  </span>
                  <span>
                    <code className="font-bold text-slate-800 dark:text-slate-100">{name}</code>
                    <span className="block text-gray-500 dark:text-gray-400 text-sm">{help}</span>
                  </span>
                </label>
              ))}
            </div>
          </div>
        ))}
      </fieldset>

      <button className="font-bold text-slate-800 dark:text-slate-100 border-slate-500 dark:border-slate-400 pl-4 pr-4 pb-2 pt-2 border-2 bg-cyan-200 dark:bg-cyan-700 hover:bg-cyan-400 dark:hover:bg-cyan-600 active:ring-3 rounded-md">
        Create token
      </button>
      <Link to={routes.authTokens()} className="text-cyan-500 dark:text-cyan-300 font-bold ml-2">
        Cancel
      </Link>
    </form>
  );
}

/** `auth_token_create`: a token for the REST API, with its capabilities, optionally limited to one project. */
export default function AuthTokenCreate() {
  useTitle("New Auth Token");
  const loaded = useLoad(() => installationApi.newAuthToken(), []);
  return (
    <Load loaded={loaded}>
      {(options) => (
        <div className="flex items-center justify-center">
          <div className="m-4 max-w-4xl flex-auto">
            <AuthTokenForm options={options} />
          </div>
        </div>
      )}
    </Load>
  );
}
