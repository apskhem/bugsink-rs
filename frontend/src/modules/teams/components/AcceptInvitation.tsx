import { useNavigate } from "react-router";

import { LoadError } from "common/components/ErrorPage";
import { routes } from "common/constants/routes";
import { useFailure } from "modules/teams/hooks/useFailure";
import { membersApi, type GroupKind } from "services/members";
import { roleLabel } from "services/teams";

import { primaryButton, secondaryButton } from "./buttons";

type Props = {
  kind: GroupKind;
  id: string;
  name: string;
  /** "team" or "project", for the text. */
  noun: string;
  role: number;
  /** Upstream's team page has `mt-4` on the heading, its project page `my-4`. */
  headingClass: string;
  /** Where accepting leads: your membership settings. */
  acceptedUrl: string;
};

/** Upstream's `team_members_accept.html` / `project_members_accept.html`: accept (on to your settings) or decline. */
export default function AcceptInvitation({ kind, id, name, noun, role, headingClass, acceptedUrl }: Props) {
  const navigate = useNavigate();
  const { failure, fail } = useFailure();

  async function respond(accept: boolean) {
    try {
      if (accept) {
        await membersApi.accept(undefined, { params: { kind, id } });
      } else {
        await membersApi.decline(undefined, { params: { kind, id } });
      }
    } catch (error) {
      fail(error);
      return;
    }
    await navigate(accept ? acceptedUrl : routes.home());
  }

  if (failure) {
    return <LoadError error={failure.error} />;
  }

  return (
    <div className="flex items-center justify-center">
      <div className="m-4 max-w-4xl flex-auto">
        <form onSubmit={(event) => event.preventDefault()}>
          <div>
            <h1 className={headingClass}>Invitation · {name}</h1>
          </div>

          <div className="mt-4 mb-4">
            You have been invited to join the {noun} "{name}" in the role of "{roleLabel(role)}". Please confirm by
            clicking the button below.
          </div>

          <button type="button" className={primaryButton} onClick={() => void respond(true)}>
            Accept
          </button>
          <button type="button" className={`${secondaryButton} ml-2`} onClick={() => void respond(false)}>
            Decline
          </button>
        </form>
      </div>
    </div>
  );
}
