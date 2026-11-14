import { useState } from "react";
import { Link, useNavigate } from "react-router";

import { LoadError } from "common/components/ErrorPage";
import { FormErrors, SelectField } from "common/components/FormField";
import { useFailure } from "modules/teams/hooks/useFailure";
import type { FieldErrors } from "services/http";
import { membersApi, type GroupKind, type MemberUpdate } from "services/members";
import { ROLES } from "services/teams";

import { Messages, type Message } from "./Flash";
import { primaryButton } from "./buttons";

type Props = {
  kind: GroupKind;
  id: string;
  name: string;
  /** "team" or "project", for the text. */
  noun: string;
  userId: number;
  username: string;
  thisIsYou: boolean;
  /** Your own role only as an admin (or superuser); someone else's always (only admins get there). */
  editRole: boolean;
  role: number;
  /** Your own membership only. */
  sendEmailAlerts: boolean | null;
  /** The "unknown" choice: what applies when not set here. */
  emailAlertsDefault: string;
  /** Upstream: back to the list for yourself, to the members page for someone else. */
  doneUrl: string;
  messages: Message[];
};

function choiceOf(value: boolean | null): string {
  return value === null ? "unknown" : String(value);
}

function valueOf(choice: string): boolean | null {
  return choice === "unknown" ? null : choice === "true";
}

/**
 * Upstream's `team_member_settings.html` / `project_member_settings.html` with `My<Team|Project>MembershipForm`
 * (yourself: email alerts, and the role as an admin) or `<Team|Project>MembershipForm` (someone else: the role).
 */
export default function MemberSettings(props: Props) {
  const { kind, id, name, noun, userId, username, thisIsYou, editRole, emailAlertsDefault, doneUrl } = props;
  const navigate = useNavigate();
  const { failure, fieldErrors } = useFailure();
  const [role, setRole] = useState(String(props.role));
  const [sendEmailAlerts, setSendEmailAlerts] = useState(choiceOf(props.sendEmailAlerts));
  const [errors, setErrors] = useState<FieldErrors>({});

  async function save() {
    const update: MemberUpdate = {
      ...(editRole ? { role: Number(role) } : {}),
      ...(thisIsYou ? { send_email_alerts: valueOf(sendEmailAlerts) } : {})
    };
    try {
      await membersApi.updateMember(update, { params: { kind, id, userId: String(userId) } });
    } catch (error) {
      setErrors(fieldErrors(error));
      return;
    }
    await navigate(doneUrl);
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
          <Messages messages={props.messages} />

          <div>
            <h1 className="text-4xl my-4 font-bold">Membership settings · {name}</h1>
          </div>

          <div className="mt-4 mb-4">
            {thisIsYou
              ? `Your membership settings for ${noun} "${name}".`
              : `Settings for ${noun} "${name}" and user ${username}.`}
          </div>

          <FormErrors errors={errors["__all__"]} />

          {editRole && (
            <SelectField
              label="Role"
              name="role"
              required
              value={role}
              onChange={(event) => setRole(event.target.value)}
              errors={errors["role"]}
            >
              {ROLES.map((choice) => (
                <option key={choice.value} value={choice.value}>
                  {choice.label}
                </option>
              ))}
            </SelectField>
          )}
          {thisIsYou && (
            <SelectField
              label="Send email alerts"
              name="send_email_alerts"
              value={sendEmailAlerts}
              onChange={(event) => setSendEmailAlerts(event.target.value)}
              errors={errors["send_email_alerts"]}
            >
              <option value="unknown">{emailAlertsDefault}</option>
              <option value="true">Yes</option>
              <option value="false">No</option>
            </SelectField>
          )}

          <button type="submit" className={primaryButton}>
            Save
          </button>
          <Link to={doneUrl} className="text-cyan-500 dark:text-cyan-300 font-bold ml-2">
            Cancel
          </Link>
        </form>
      </div>
    </div>
  );
}
