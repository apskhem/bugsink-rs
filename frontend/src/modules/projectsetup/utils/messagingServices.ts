import dayjs from "dayjs";
import utc from "dayjs/plugin/utc";

dayjs.extend(utc);

/** Django's `date` filter with `format` (a dayjs format), in the installation's time zone (UTC). */
export function formatDate(timestamp: string, format: string): string {
  return dayjs.utc(timestamp).format(format);
}

/** The model's `display_name` help text. */
export const DISPLAY_NAME_HELP = 'For display in the UI, e.g. "#general on company Slack"';

export const saveButton =
  "font-bold text-slate-800 dark:text-slate-100 border-slate-500 dark:border-slate-400 pl-4 pr-4 pb-2 pt-2 border-2 bg-cyan-200 dark:bg-cyan-700 hover:bg-cyan-400 dark:hover:bg-cyan-600 active:ring-3 rounded-md";

export type ConfigField = {
  name: string;
  label: string;
  type: "url" | "text" | "number";
  required: boolean;
  helpText?: string;
  maxLength?: number;
  min?: number;
};

const webhookUrl: ConfigField = { name: "webhook_url", label: "Webhook url", type: "url", required: true };

/** Each kind's config form (`get_form_class()`), with Django's default labels. */
export function configFields(kind: string): ConfigField[] {
  switch (kind) {
    case "msteams":
      // Teams workflow URLs are long
      return [{ ...webhookUrl, maxLength: 1000 }];
    case "mattermost":
      return [
        webhookUrl,
        {
          name: "channel",
          label: "Channel",
          type: "text",
          required: false,
          helpText: 'Optional: Override channel (e.g., "town-square" or "@username" for DMs)'
        }
      ];
    case "telegram":
      return [
        {
          name: "bot_token",
          label: "Bot token",
          type: "text",
          required: true,
          helpText: "Telegram bot token (from https://t.me/BotFather)"
        },
        {
          name: "chat_id",
          label: "Chat id",
          type: "text",
          required: true,
          helpText: 'Chat ID or channel username (e.g. "-1001234567890" or "@channelusername")'
        },
        {
          name: "message_thread_id",
          label: "Message thread id",
          type: "number",
          required: false,
          min: 1,
          helpText: "Optional: Topic ID for Telegram groups with topics enabled"
        }
      ];
    default:
      return [webhookUrl];
  }
}

/** The form's values for a stored config: its fields as text (null and absent as empty). */
export function configValues(kind: string, config: unknown): Record<string, string> {
  const stored = typeof config === "object" && config !== null ? new Map(Object.entries(config)) : new Map();
  return Object.fromEntries(
    configFields(kind).map(({ name }) => {
      const value: unknown = stored.get(name);
      return [name, value === null || value === undefined ? "" : String(value)];
    })
  );
}
