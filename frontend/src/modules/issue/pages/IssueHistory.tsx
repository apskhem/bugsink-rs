import { useReducer, useState, type KeyboardEvent } from "react";
import { Link, useParams } from "react-router";

import Load from "common/components/Load";
import { routes } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { useUser } from "contexts/session";
import IssueLayout from "modules/issue/components/IssueLayout";
import { LinebreaksBr } from "modules/issue/components/Text";
import { useGravatarSha, useLastEvent, useQ } from "modules/issue/hooks";
import { dateTime } from "modules/issue/utils/dates";
import { asRecord, djStr, truthy } from "modules/issue/utils/python";
import { apiError } from "services/http";
import { issueApi, type TurningPoint } from "services/issue";

/** `TurningPointKind` */
const KINDS: Record<number, string> = {
  1: "First seen",
  2: "Resolved",
  3: "Muted",
  4: "Marked as regressed",
  5: "Unmuted",
  6: "Reopened",
  10: "Release info added",
  100: "Manual annotation"
};

const TEXTAREA =
  "bg-slate-50 dark:bg-slate-800 focus:border-cyan-500 dark:focus:border-cyan-400 focus:ring-cyan-200 dark:focus:ring-cyan-700 rounded-md w-full h-32";
const BUTTON =
  "font-bold text-slate-500 dark:text-slate-300 border-slate-300 dark:border-slate-600 pl-4 pr-4 pb-2 pt-2 mt-2 border-2 rounded-md hover:bg-slate-200 dark:hover:bg-slate-800 active:ring-3";

/** `issue_history.js`'s `submitOnCtrlEnter` */
function submitOnCtrlEnter(event: KeyboardEvent<HTMLTextAreaElement>) {
  if (event.key === "Enter" && event.ctrlKey) {
    event.preventDefault();
    event.currentTarget.form?.requestSubmit();
  }
}

const gravatar = (sha: string) => `https://gravatar.com/avatar/${sha}?s=48&d=robohash`;
const AVATAR = "w-12 h-12 rounded-full border-2 border-slate-300 dark:border-slate-600";
const plural = (n: unknown) => (n !== 1 ? "s" : "");
const dateOf = (value: unknown) => (typeof value === "string" ? dateTime(value) : djStr(value));

/** What upstream says about a turning point's metadata. */
function Metadata({ kind, metadata }: { kind: number; metadata: Record<string, unknown> }) {
  const muteUntil = asRecord(metadata["mute_until"]);
  const muteFor = asRecord(metadata["mute_for"]);
  return (
    <div className="mt-4">
      {kind === 3 && (
        <>
          {truthy(metadata["mute_unconditionally"]) && "Muted unconditionally. "}
          {muteUntil &&
            `Muted until more than ${djStr(muteUntil["gte_threshold"])} events per ${djStr(muteUntil["nr_of_periods"])} ${djStr(muteUntil["period_name"])}${plural(muteUntil["nr_of_periods"])} occur. `}
          {muteFor &&
            `Muted for ${djStr(muteFor["nr_of_periods"])} ${djStr(muteFor["period_name"])}${plural(muteFor["nr_of_periods"])}, i.e. until ${dateOf(muteFor["unmute_after"])}. `}
        </>
      )}
      {kind === 5 && (
        <>
          {muteUntil &&
            `More than ${djStr(muteUntil["volume"])} events per ${djStr(muteUntil["nr_of_periods"])} ${djStr(muteUntil["period"])}${plural(muteUntil["nr_of_periods"])} occurred, unmuting the issue. `}
          {muteFor &&
            `An event was observed after the mute-deadline of ${dateOf(muteFor["unmute_after"])} and the issue was unmuted. `}
        </>
      )}
      {truthy(metadata["resolved_unconditionally"]) && "Marked as 'resolved' (without a specific release-marker). "}
      {truthy(metadata["resolved_release"]) &&
        `Marked as 'resolved in release ${djStr(metadata["resolved_release"])}'. `}
      {truthy(metadata["resolve_by_next"]) && "Marked as 'resolved by next release'. "}
      {truthy(metadata["actual_release"]) &&
        `A new version was released (${djStr(metadata["actual_release"])}) and the issue was marked as resolved by that specific release.`}
    </div>
  );
}

type EntryProps = {
  issueId: string;
  turningPoint: TurningPoint;
  mine: boolean;
  onChanged: () => void;
};

/** A turning point's "balloon": header, comment (editable when it's yours), metadata, triggering event. */
function TurningPointEntry({ issueId, turningPoint: tp, mine, onChanged }: EntryProps) {
  const [editing, setEditing] = useState(false);
  const [comment, setComment] = useState(tp.comment);
  const metadata = asRecord(tp.metadata);
  const hasMetadata = truthy(metadata);

  async function update() {
    try {
      await issueApi.editComment({ comment }, { params: { issueId, commentId: tp.id } });
      setEditing(false);
      onChanged();
    } catch (error) {
      window.alert(apiError(error)?.detail ?? String(error));
    }
  }

  async function remove() {
    if (window.confirm("Are you sure you want to delete this comment?")) {
      try {
        await issueApi.deleteComment(undefined, { params: { issueId, commentId: tp.id } });
      } finally {
        onChanged();
      }
    }
  }

  return (
    <div className="flex" id={`comment-${tp.id}`}>
      <div className="flex-none">
        <div className="pt-8 pr-2">
          {tp.user ? (
            <img className={AVATAR} src={gravatar(tp.user.gravatar_sha256)} alt={tp.user.display_name} />
          ) : (
            <img className={AVATAR} src="/images/bugsink-logo.png" alt="Bugsink" />
          )}
        </div>
      </div>

      <div className="border-slate-300 dark:border-slate-600 border-2 rounded-md mt-6 flex-auto">
        <div className="pl-4 flex triangle-left">
          <div className="mt-4 mb-4">
            <span className="font-bold text-slate-800 dark:text-slate-100">{KINDS[tp.kind] ?? String(tp.kind)}</span> by{" "}
            <span className="font-bold text-slate-800 dark:text-slate-100">
              {tp.user ? tp.user.display_name : "Bugsink"}
            </span>
            {mine && (
              <>
                {" "}
                <span
                  className="text-slate-500 dark:text-slate-300 pl-1"
                  onClick={() => setEditing((e) => !e)}
                  title="Edit comment"
                >
                  <svg
                    xmlns="http://www.w3.org/2000/svg"
                    viewBox="0 0 16 16"
                    fill="currentColor"
                    className="w-4 h-4 inline"
                  >
                    <path
                      fillRule="evenodd"
                      d="M11.013 2.513a1.75 1.75 0 0 1 2.475 2.474L6.226 12.25a2.751 2.751 0 0 1-.892.596l-2.047.848a.75.75 0 0 1-.98-.98l.848-2.047a2.75 2.75 0 0 1 .596-.892l7.262-7.261Z"
                      clipRule="evenodd"
                    />
                  </svg>
                </span>
                {tp.kind === 100 && (
                  <>
                    {" "}
                    <span
                      className="text-slate-500 dark:text-slate-300 pl-1"
                      onClick={() => void remove()}
                      title="Delete comment"
                    >
                      <svg
                        xmlns="http://www.w3.org/2000/svg"
                        viewBox="0 0 16 16"
                        fill="currentColor"
                        className="w-4 h-4 inline"
                      >
                        <path
                          fillRule="evenodd"
                          d="M5 3.25V4H2.75a.75.75 0 0 0 0 1.5h.3l.815 8.15A1.5 1.5 0 0 0 5.357 15h5.285a1.5 1.5 0 0 0 1.493-1.35l.815-8.15h.3a.75.75 0 0 0 0-1.5H11v-.75A2.25 2.25 0 0 0 8.75 1h-1.5A2.25 2.25 0 0 0 5 3.25Zm2.25-.75a.75.75 0 0 0-.75.75V4h3v-.75a.75.75 0 0 0-.75-.75h-1.5ZM6.05 6a.75.75 0 0 1 .787.713l.275 5.5a.75.75 0 0 1-1.498.075l-.275-5.5A.75.75 0 0 1 6.05 6Zm3.9 0a.75.75 0 0 1 .712.787l-.275 5.5a.75.75 0 0 1-1.498-.075l.275-5.5a.75.75 0 0 1 .786-.711Z"
                          clipRule="evenodd"
                        />
                      </svg>
                    </span>
                  </>
                )}
              </>
            )}
          </div>

          <div className="ml-auto flex">
            <div className="p-4 text-right">{dateTime(tp.timestamp)}</div>
          </div>
        </div>

        {(hasMetadata || tp.triggering_event || tp.comment || mine) && (
          <div className="border-t-2 pl-4 pr-4 pb-4 border-slate-300 dark:border-slate-600">
            <div className="mt-4">
              <div className={editing ? "hidden" : undefined}>
                <LinebreaksBr text={tp.comment} />
              </div>

              {mine && (
                <div className={editing ? undefined : "hidden"}>
                  <form
                    onSubmit={(event) => {
                      event.preventDefault();
                      void update();
                    }}
                  >
                    <textarea
                      name="comment"
                      placeholder="comments..."
                      className={TEXTAREA}
                      value={comment}
                      onChange={(event) => setComment(event.target.value)}
                      onKeyDown={submitOnCtrlEnter}
                    />
                    <button className={BUTTON}>Update comment</button>
                  </form>
                </div>
              )}
            </div>

            {metadata && hasMetadata && <Metadata kind={tp.kind} metadata={metadata} />}

            {tp.triggering_event && (
              <div className="mt-4">
                <Link
                  to={routes.eventById(tp.triggering_event.id)}
                  className="underline decoration-dotted font-bold text-slate-500 dark:text-slate-300"
                >
                  Triggering event
                </Link>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

/** The "your comments" balloon at the top. */
function NewComment({ issueId, onChanged }: { issueId: string; onChanged: () => void }) {
  const user = useUser();
  const sha = useGravatarSha(user.email);
  const [comment, setComment] = useState("");
  const displayName = [user.first_name, user.last_name].filter(Boolean).join(" ") || user.username;

  async function post() {
    try {
      await issueApi.addComment({ comment }, { params: { issueId } });
      setComment("");
      onChanged();
    } catch (error) {
      window.alert(apiError(error)?.detail ?? String(error));
    }
  }

  return (
    <div className="flex">
      <div className="flex-none">
        <div className="pt-8 pr-2">
          <img className={AVATAR} src={gravatar(sha)} alt={displayName} />
        </div>
      </div>
      <div className="border-slate-300 dark:border-slate-600 border-2 rounded-md mt-6 flex-auto">
        <div className="pl-4 flex triangle-left">
          <div className="mt-4 mb-4">
            <span className="font-bold text-slate-800 dark:text-slate-100 italic">
              Add comment as manual annotation
            </span>
          </div>
          <div className="ml-auto flex">
            <div className="p-4">Now</div>
          </div>
        </div>

        <div className="border-t-2 pl-4 pr-4 pb-4 border-slate-300 dark:border-slate-600">
          <div className="mt-4">
            <form
              onSubmit={(event) => {
                event.preventDefault();
                void post();
              }}
            >
              <textarea
                name="comment"
                placeholder="comments..."
                className={TEXTAREA}
                value={comment}
                onChange={(event) => setComment(event.target.value)}
                onKeyDown={submitOnCtrlEnter}
              />
              <button className={BUTTON}>Post comment</button>
            </form>
          </div>
        </div>
      </div>
    </div>
  );
}

/** Upstream's `issues/history.html` with `issue_history.js`: turning points, newest first, and comments. */
export default function IssueHistory() {
  const { issueId = "" } = useParams();
  const q = useQ();
  const user = useUser();
  const [version, bump] = useReducer((n: number) => n + 1, 0);
  const last = useLastEvent(issueId, q, version);
  const history = useLoad(() => issueApi.history({ params: { issueId } }), [issueId, version]);

  return (
    <IssueLayout
      issueId={issueId}
      tab="history"
      version={version}
      onChanged={bump}
      event={null}
      requestRepr={last.data?.display?.request_repr ?? ""}
    >
      {() => (
        <>
          <h1 className="text-2xl font-bold text-ellipsis whitespace-nowrap overflow-hidden">History</h1>
          <div className="italic">Most recent first</div>

          <NewComment issueId={issueId} onChanged={bump} />

          <Load loaded={history}>
            {(turningPoints) =>
              turningPoints.map((tp) => (
                <TurningPointEntry
                  key={`${tp.id}-${tp.comment}`}
                  issueId={issueId}
                  turningPoint={tp}
                  mine={tp.user?.id === user.id}
                  onChanged={bump}
                />
              ))
            }
          </Load>
        </>
      )}
    </IssueLayout>
  );
}
