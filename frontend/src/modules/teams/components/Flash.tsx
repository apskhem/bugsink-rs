import { useLocation } from "react-router";
import { z } from "zod";

import { CopyButton } from "common/components/CopyButton";

/**
 * Upstream's Django `messages` and `invite_link_notice`, shown on the page a view redirects (or renders) to: here
 * they travel in the navigation's state.
 */
const Message = z.object({ level: z.enum(["success", "warning"]), text: z.string() });
export type Message = z.infer<typeof Message>;

const InviteLinkNoticeData = z.object({
  /** "email_not_sent": right after inviting; "manual": "Show invite link". */
  kind: z.enum(["email_not_sent", "manual"]),
  link: z.string(),
  email: z.string()
});
export type InviteLinkNoticeData = z.infer<typeof InviteLinkNoticeData>;

const FlashState = z.object({
  messages: z.array(Message).default([]),
  inviteLinkNotice: InviteLinkNoticeData.nullable().default(null)
});
export type Flash = z.output<typeof FlashState>;

export function success(text: string): Message {
  return { level: "success", text };
}

export function warning(text: string): Message {
  return { level: "warning", text };
}

/** The navigation state that shows these on the next page. */
export function flash(messages: Message[], inviteLinkNotice: InviteLinkNoticeData | null = null): Flash {
  return { messages, inviteLinkNotice };
}

export function useFlash(): Flash {
  const { state } = useLocation();
  const parsed = FlashState.safeParse(state ?? {});
  return parsed.success ? parsed.data : { messages: [], inviteLinkNotice: null };
}

const messageClass = {
  success: "bg-cyan-50 dark:bg-cyan-900 border-2 border-cyan-800 dark:border-cyan-400 p-4 rounded-lg",
  warning: "bg-yellow-100 dark:bg-amber-800 border-2 border-yellow-200 dark:border-amber-900 p-4 rounded-lg"
};

export function Messages({ messages, className }: { messages: Message[]; className?: string }) {
  if (!messages.length) {
    return null;
  }
  return (
    <ul className={className}>
      {messages.map((message) => (
        <li key={message.text} className={messageClass[message.level]}>
          {message.text}
        </li>
      ))}
    </ul>
  );
}

/** Upstream's `includes/invite_link_notice.html`. */
export function InviteLinkNotice({ notice }: { notice: InviteLinkNoticeData | null }) {
  if (!notice) {
    return null;
  }
  return (
    <div className="mb-4 bg-cyan-50 dark:bg-cyan-900 border-2 border-cyan-800 dark:border-cyan-400 p-4 rounded-lg">
      {notice.kind === "email_not_sent" ? (
        <>
          <div className="font-bold">Invitation not sent</div>
          <div>
            No invitation was sent because email is not set up. Hand out the following link to {notice.email} yourself:
          </div>
        </>
      ) : (
        <div>Hand out the following link to {notice.email} yourself:</div>
      )}
      <CopyButton value={notice.link} />
    </div>
  );
}
