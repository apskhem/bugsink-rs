import { makeApi, Zodios } from "@zodios/core";
import { z } from "zod";

import { http } from "./http";
import { Me } from "./session";

/** Signup when the address must be confirmed first (201); otherwise the new user is logged in (200, `Me`). */
export const ConfirmationSent = z.object({ confirmation_sent_to: z.string() });
export type ConfirmationSent = z.infer<typeof ConfirmationSent>;

export const SignupResult = z.union([ConfirmationSent, Me]);
export type SignupResult = z.infer<typeof SignupResult>;

export const Theme = z.enum(["system", "light", "dark"]);
export type Theme = z.infer<typeof Theme>;

export const Preferences = z.object({
  send_email_alerts: z.boolean(),
  theme_preference: Theme,
  language: z.string()
});
export type Preferences = z.infer<typeof Preferences>;

const token = { name: "token", type: "Path", schema: z.string() } as const;

const endpoints = makeApi([
  {
    method: "post",
    path: "/auth/signup",
    alias: "signup",
    parameters: [{ name: "body", type: "Body", schema: z.object({ email: z.string(), password: z.string() }) }],
    response: SignupResult
  },
  {
    method: "get",
    path: "/auth/confirm-email/:token",
    alias: "checkConfirmEmailToken",
    parameters: [token],
    response: z.unknown()
  },
  {
    method: "post",
    path: "/auth/confirm-email/:token",
    alias: "confirmEmail",
    parameters: [token],
    response: Me
  },
  {
    method: "post",
    path: "/auth/resend-confirmation",
    alias: "resendConfirmation",
    parameters: [{ name: "body", type: "Body", schema: z.object({ email: z.string() }) }],
    response: z.unknown()
  },
  {
    method: "post",
    path: "/auth/request-reset-password",
    alias: "requestResetPassword",
    parameters: [{ name: "body", type: "Body", schema: z.object({ email: z.string() }) }],
    response: z.unknown()
  },
  {
    method: "get",
    path: "/auth/reset-password/:token",
    alias: "checkResetPasswordToken",
    parameters: [token],
    response: z.unknown()
  },
  {
    method: "post",
    path: "/auth/reset-password/:token",
    alias: "resetPassword",
    parameters: [token, { name: "body", type: "Body", schema: z.object({ password: z.string() }) }],
    response: Me
  },
  {
    method: "patch",
    path: "/me",
    alias: "updatePreferences",
    parameters: [{ name: "body", type: "Body", schema: Preferences.partial() }],
    response: z.unknown()
  },
  {
    method: "post",
    path: "/me/password",
    alias: "changePassword",
    parameters: [
      { name: "body", type: "Body", schema: z.object({ old_password: z.string(), new_password: z.string() }) }
    ],
    response: z.unknown()
  }
]);

export const accountsApi = new Zodios("/api/v1", endpoints, { axiosInstance: http });
