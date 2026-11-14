import { useEffect, useState, type FormEvent } from "react";
import { Link, useLocation, useNavigate } from "react-router";
import { z } from "zod";

import { FormErrors, SelectField } from "common/components/FormField";
import Messages from "common/components/Messages";
import { routes } from "common/constants/routes";
import { useTitle } from "common/hooks/useTitle";
import { useSession, useUser } from "contexts/session";
import { accountsApi, Theme } from "services/accounts";
import type { FieldErrors } from "services/http";

import { submitErrors } from "../utils/errors";

/** Upstream's `User.THEME_CHOICES`. */
const THEMES: [Theme, string][] = [
  ["system", "System Default"],
  ["light", "Light"],
  ["dark", "Dark"]
];

/** Upstream's `language_choices()` (its `LANGUAGES`, labelled as Django does in English). */
const LANGUAGES: [string, string][] = [
  ["auto", "Auto (browser preference)"],
  ["en", "English"],
  ["zh-hans", "简体中文 (Simplified Chinese)"]
];

/** A message for this page from the one before (upstream's `messages` after a redirect, e.g. "Password updated"). */
const FlashState = z.object({ message: z.string() });

/** Upstream's `users/preferences.html`: email alerts, theme and language. */
export default function Preferences() {
  useTitle("User Preferences");
  const user = useUser();
  const { refresh } = useSession();
  const location = useLocation();
  const navigate = useNavigate();
  const [messages, setMessages] = useState<string[]>(() => {
    const flash = FlashState.safeParse(location.state);
    return flash.success ? [flash.data.message] : [];
  });
  const [sendEmailAlerts, setSendEmailAlerts] = useState(user.send_email_alerts);
  const [theme, setTheme] = useState<Theme>(user.theme_preference);
  const [language, setLanguage] = useState(user.language);
  const [errors, setErrors] = useState<FieldErrors>({});

  // like Django's messages, shown once: not again after a reload
  useEffect(() => {
    if (location.state) {
      void navigate(location.pathname, { replace: true, state: null });
    }
  }, [location.state, location.pathname, navigate]);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    try {
      await accountsApi.updatePreferences({
        send_email_alerts: sendEmailAlerts,
        theme_preference: theme,
        language
      });
      setErrors({});
      setMessages(["Updated preferences"]);
      await refresh(); // applies the theme
    } catch (error) {
      setMessages([]);
      setErrors(submitErrors(error));
    }
  }

  return (
    <div className="flex items-center justify-center">
      <div className="m-4 max-w-4xl flex-auto">
        <form method="post" onSubmit={(event) => void submit(event)}>
          <Messages messages={messages} />

          <div>
            <h1 className="text-4xl my-4 font-bold">User Preferences</h1>
          </div>

          <FormErrors errors={errors["__all__"]} />
          <SelectField
            label="Send email alerts"
            errors={errors["send_email_alerts"]}
            name="send_email_alerts"
            id="id_send_email_alerts"
            value={sendEmailAlerts ? "True" : "False"}
            onChange={(event) => setSendEmailAlerts(event.target.value === "True")}
          >
            <option value="True">Yes</option>
            <option value="False">No</option>
          </SelectField>
          <SelectField
            label="Theme preference"
            errors={errors["theme_preference"]}
            name="theme_preference"
            id="id_theme_preference"
            value={theme}
            onChange={(event) => {
              const chosen = Theme.safeParse(event.target.value);
              if (chosen.success) {
                setTheme(chosen.data);
              }
            }}
          >
            {THEMES.map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </SelectField>
          <SelectField
            label="Language"
            errors={errors["language"]}
            name="language"
            id="id_language"
            value={language}
            onChange={(event) => setLanguage(event.target.value)}
          >
            {LANGUAGES.map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </SelectField>

          <button
            name="action"
            value="preferences"
            className="font-bold text-slate-800 dark:text-slate-100 border-slate-500 dark:border-slate-400 pl-4 pr-4 pb-2 pt-2 border-2 bg-cyan-200 dark:bg-cyan-700 hover:bg-cyan-400 dark:hover:bg-cyan-600 active:ring-3 rounded-md"
          >
            Save
          </button>
        </form>

        <div className="mt-8 text-sm">
          <Link to={routes.changePassword()} className="text-cyan-500 dark:text-cyan-300 font-bold">
            Change password
          </Link>
        </div>
      </div>
    </div>
  );
}
