import { useState, type ReactNode } from "react";
import { Link, useNavigate } from "react-router";

import { LoadError } from "common/components/ErrorPage";
import { routes } from "common/constants/routes";
import { useUser } from "contexts/session";
import { useFailure } from "modules/teams/hooks/useFailure";
import { apiError } from "services/http";
import { membersApi, type GroupKind, type Member, type MemberList } from "services/members";
import { ROLE_ADMIN } from "services/teams";

import { LeaveTeamModal } from "./ConfirmModal";
import { flash, InviteLinkNotice, Messages, success, warning, type Flash } from "./Flash";
import { primaryLink, secondaryButton } from "./buttons";

type Props = {
  kind: GroupKind;
  /** The team's or project's id, as the API takes it. */
  id: string;
  name: string;
  list: MemberList;
  /** What the previous page passed on (messages, an invite link to hand out). */
  initial: Flash;
  inviteUrl: string;
  memberSettingsUrl: (userId: number) => string;
  /** Upstream's team page has a dark variant for the member links, its project page doesn't. */
  memberLinkClass: string;
  footer: ReactNode;
};

const rowButton = `${secondaryButton} ml-2 whitespace-nowrap`;

/**
 * Upstream's `team_members.html` / `project_members.html`: the members with their invitations' state, and per member
 * "Show invite link" (without email) or "Reinvite" (with), and "Remove" (yourself: "Leave"; for teams after
 * confirming, and then on to the team list).
 */
export default function Members(props: Props) {
  const { kind, id, name, list, initial, inviteUrl, memberSettingsUrl, memberLinkClass, footer } = props;
  const user = useUser();
  const navigate = useNavigate();
  const { failure, fail } = useFailure();
  const [members, setMembers] = useState<Member[]>(list.members);
  const [messages, setMessages] = useState(initial.messages);
  const [notice, setNotice] = useState(initial.inviteLinkNotice);
  const [leaving, setLeaving] = useState(false);

  const params = { kind, id };

  /** Each action re-renders upstream's page: with only that action's message or notice. */
  function show(next: Flash) {
    setMessages(next.messages);
    setNotice(next.inviteLinkNotice);
  }

  async function remove(member: Member) {
    try {
      await membersApi.removeMember(undefined, { params: { ...params, userId: String(member.user_id) } });
    } catch (error) {
      fail(error);
      return;
    }
    setMembers((current) => current.filter((m) => m.user_id !== member.user_id));
    show(flash([]));
  }

  async function leaveTeam() {
    try {
      await membersApi.removeMember(undefined, { params: { ...params, userId: String(user.id) } });
      await navigate(routes.teams("mine"), { state: flash([success(`You have left the team "${name}"`)]) });
    } catch (error) {
      const failed = apiError(error);
      if (failed?.status === 409) {
        await navigate(routes.teams("mine"), { state: flash([warning(failed.detail)]) });
      } else {
        fail(error);
      }
    }
  }

  async function showInviteLink(member: Member) {
    try {
      const { invite_link } = await membersApi.inviteLink({
        params: { ...params, userId: String(member.user_id) }
      });
      show(flash([], { kind: "manual", link: invite_link, email: member.email }));
    } catch (error) {
      fail(error);
    }
  }

  async function reinvite(member: Member) {
    try {
      const { email } = await membersApi.reinvite(undefined, {
        params: { ...params, userId: String(member.user_id) }
      });
      show(flash([success(`Invitation resent to ${email}`)]));
    } catch (error) {
      fail(error);
    }
  }

  if (failure) {
    return <LoadError error={failure.error} />;
  }

  return (
    <>
      {leaving && <LeaveTeamModal onCancel={() => setLeaving(false)} onConfirm={() => void leaveTeam()} />}

      <div className="flex items-center justify-center">
        <div className="m-4 max-w-4xl flex-auto">
          <Messages messages={messages} className="mb-4" />

          <InviteLinkNotice notice={notice} />

          <div className="flex">
            <h1 className="text-4xl mt-4 font-bold">Members · {name}</h1>

            <div className="ml-auto mt-6">
              <Link className={primaryLink} to={inviteUrl}>
                Invite Member
              </Link>
            </div>
          </div>

          <div>
            <table className="w-full mt-8">
              <thead>
                <tr className="bg-slate-200 dark:bg-slate-800">
                  <th className="w-full p-4 text-left text-xl" colSpan={2}>
                    {name}
                  </th>
                </tr>
              </thead>
              <tbody>
                {members.map((member) => (
                  <tr
                    key={member.user_id}
                    className="bg-white dark:bg-slate-900 border-slate-200 dark:border-slate-700 border-b-2"
                  >
                    <td className="w-full p-4">
                      <div>
                        <Link to={memberSettingsUrl(member.user_id)} className={memberLinkClass}>
                          {member.email}
                        </Link>
                        {!member.accepted ? (
                          <span className="bg-slate-100 dark:bg-slate-700 rounded-2xl px-4 py-2 ml-2 text-sm">
                            Invitation pending
                          </span>
                        ) : (
                          member.role === ROLE_ADMIN && (
                            <span className="bg-cyan-100 dark:bg-cyan-900 rounded-2xl px-4 py-2 ml-2 text-sm">
                              Admin
                            </span>
                          )
                        )}
                      </div>
                    </td>

                    <td className="p-4">
                      <div className="flex justify-end">
                        {!member.accepted &&
                          (!list.email_delivers && member.is_active ? (
                            <button type="button" className={rowButton} onClick={() => void showInviteLink(member)}>
                              Show invite link
                            </button>
                          ) : list.email_delivers ? (
                            <button type="button" className={rowButton} onClick={() => void reinvite(member)}>
                              Reinvite
                            </button>
                          ) : (
                            <button
                              type="button"
                              disabled
                              className="font-bold text-slate-400 dark:text-slate-600 border-slate-200 dark:border-slate-800 pl-4 pr-4 pb-2 pt-2 ml-2 border-2 rounded-md whitespace-nowrap cursor-not-allowed"
                            >
                              Reinvite
                            </button>
                          ))}
                        {member.user_id === user.id ? (
                          <button
                            type="button"
                            className={rowButton}
                            onClick={() => (kind === "teams" ? setLeaving(true) : void remove(member))}
                          >
                            Leave
                          </button>
                        ) : (
                          <button type="button" className={rowButton} onClick={() => void remove(member)}>
                            Remove
                          </button>
                        )}
                      </div>
                    </td>
                  </tr>
                ))}
                {!members.length && (
                  <tr className="bg-white dark:bg-slate-900 border-slate-200 dark:border-slate-700 border-b-2">
                    <td className="w-full p-4">
                      <div>
                        No members yet.{" "}
                        <Link to={inviteUrl} className="text-cyan-500 dark:text-cyan-300 font-bold">
                          Invite someone
                        </Link>
                        .
                      </div>
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>

          <div className="flex flex-direction-row">
            <div className="ml-auto py-8 pr-4">{footer}</div>
          </div>
        </div>
      </div>
    </>
  );
}
