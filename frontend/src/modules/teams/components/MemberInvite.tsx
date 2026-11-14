import { useRef, useState } from "react";
import { Link, useNavigate } from "react-router";

import { LoadError } from "common/components/ErrorPage";
import { FormErrors, InputField } from "common/components/FormField";
import { useFailure } from "modules/teams/hooks/useFailure";
import type { FieldErrors } from "services/http";
import { membersApi, type GroupKind } from "services/members";
import { ROLES } from "services/teams";

import { flash, InviteLinkNotice, Messages, success, type Flash } from "./Flash";
import { primaryButton, secondaryButton } from "./buttons";

type Props = {
  kind: GroupKind;
  id: string;
  name: string;
  /** "team" or "project", for the text. */
  noun: string;
  /** Upstream's team page has `mt-4` on the heading, its project page `my-4`. */
  headingClass: string;
  membersUrl: string;
  initial: Flash;
};

/** The message upstream shows after inviting (when there's no link to hand out). */
function invitedMessage(email: string, created: boolean, emailed: boolean): string {
  if (created) {
    return emailed
      ? `Invitation sent to ${email}`
      : `Invitation created for ${email}, but no email was sent because email is not set up.`;
  }
  return emailed
    ? `Invitation resent to ${email} (it was previously sent and we just sent it again)`
    : `Invitation already exists for ${email}, but no email was sent because email is not set up.`;
}

/**
 * Upstream's `team_members_invite.html` / `project_members_invite.html`: an email address and a role; "Invite and add
 * another" stays on the page. Without email set up, the link to hand out is shown (on the members page, or here).
 */
export default function MemberInvite({ kind, id, name, noun, headingClass, membersUrl, initial }: Props) {
  const navigate = useNavigate();
  const { failure, fieldErrors } = useFailure();
  const [email, setEmail] = useState("");
  const [role, setRole] = useState<number>(0);
  const [errors, setErrors] = useState<FieldErrors>({});
  const [messages, setMessages] = useState(initial.messages);
  const [notice, setNotice] = useState(initial.inviteLinkNotice);
  const another = useRef(false);

  /** "Invite and add another": the page again, with a fresh form. */
  function again(next: Flash) {
    setEmail("");
    setRole(0);
    setErrors({});
    setMessages(next.messages);
    setNotice(next.inviteLinkNotice);
  }

  async function invite() {
    const address = email.trim();
    let invited;
    try {
      invited = await membersApi.invite({ email: address, role }, { params: { kind, id } });
    } catch (error) {
      setErrors(fieldErrors(error));
      return;
    }
    const next =
      invited.invite_link !== null
        ? flash([], { kind: "email_not_sent", link: invited.invite_link, email: address })
        : flash([success(invitedMessage(address, invited.membership_created, invited.emailed))]);
    if (another.current) {
      again(next);
    } else {
      await navigate(membersUrl, { state: next });
    }
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
            void invite();
          }}
        >
          <Messages messages={messages} />

          <InviteLinkNotice notice={notice} />

          <div>
            <h1 className={headingClass}>Invite members · {name}</h1>
          </div>

          <div className="mt-4 mb-4">
            Invite a member to join the {noun} "{name}". They will receive an email with a link to join.
          </div>

          <FormErrors errors={errors["__all__"]} />

          <InputField
            label="Email"
            implicit
            type="email"
            name="email"
            maxLength={320}
            required
            value={email}
            onChange={(event) => setEmail(event.target.value)}
            errors={errors["email"]}
          />

          <div className="text-lg ml-1 mb-8">
            <div className="text-slate-800 dark:text-slate-100 font-bold">Role</div>
            <div className="flex items-center">
              <div>
                {ROLES.map((choice) => (
                  <div key={choice.value}>
                    <label>
                      <input
                        type="radio"
                        name="role"
                        value={choice.value}
                        required
                        checked={role === choice.value}
                        onChange={() => setRole(choice.value)}
                      />{" "}
                      {choice.label}
                    </label>
                  </div>
                ))}
              </div>
            </div>
            {errors["role"]?.map((error) => (
              <div key={error} className="text-red-500 dark:text-red-400 pt-1 px-2 text-sm">
                {error}
              </div>
            ))}
          </div>

          <button
            type="submit"
            className={primaryButton}
            onClick={() => {
              another.current = false;
            }}
          >
            Invite Member
          </button>
          <button
            type="submit"
            className={`${secondaryButton} ml-2`}
            onClick={() => {
              another.current = true;
            }}
          >
            Invite and add another
          </button>
          <Link to={membersUrl} className="font-bold text-slate-500 dark:text-slate-300 ml-4">
            Cancel
          </Link>
        </form>
      </div>
    </div>
  );
}
