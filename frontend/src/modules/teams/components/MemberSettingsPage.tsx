import type { ComponentProps } from "react";
import { Navigate } from "react-router";

import { Forbidden, NotFound } from "common/components/ErrorPage";
import Load from "common/components/Load";
import type { Loaded } from "common/hooks/useLoad";

import MemberSettings from "./MemberSettings";

/** What loading a member settings page comes to: upstream's redirect or error, or the form. */
export type MemberSettingsData =
  | { go: "accept"; to: string }
  | { go: "forbidden"; message: string }
  | { go: "notFound" }
  | { go: "form"; form: ComponentProps<typeof MemberSettings> };

/** Renders a member settings page once loaded (shared by teams and projects). */
export default function MemberSettingsPage({ loaded }: { loaded: Loaded<MemberSettingsData> }) {
  return (
    <Load loaded={loaded}>
      {(data) => {
        switch (data.go) {
          case "accept":
            return <Navigate to={data.to} replace />;
          case "forbidden":
            return <Forbidden message={data.message} />;
          case "notFound":
            return <NotFound />;
          case "form":
            return <MemberSettings key={`${data.form.id}/${data.form.userId}`} {...data.form} />;
        }
      }}
    </Load>
  );
}
