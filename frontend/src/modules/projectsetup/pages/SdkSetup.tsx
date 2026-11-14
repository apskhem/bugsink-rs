import type { ReactNode } from "react";
import { useParams } from "react-router";

import { NotFound } from "common/components/ErrorPage";
import Load from "common/components/Load";
import { useLoad } from "common/hooks/useLoad";
import { useTitle } from "common/hooks/useTitle";
import { useSetNavProject } from "common/layouts/NavProject";
import {
  GenericSetup,
  GoSetup,
  isPlatform,
  JavaScriptSetup,
  PhpSetup,
  PLATFORMS,
  PythonSetup,
  type Platform,
  type SdkProps
} from "modules/projectsetup/components/SdkInstructions";
import { useIntParam } from "modules/projectsetup/hooks/useIntParam";
import { projectName, projectSetupApi } from "services/projectsetup";

const PAGES: Record<Platform, (props: SdkProps) => ReactNode> = {
  "": GenericSetup,
  "python": PythonSetup,
  "javascript": JavaScriptSetup,
  "php": PhpSetup,
  "go": GoSetup
};

function SdkSetupPage({ projectId, platform }: { projectId: number; platform: Platform }) {
  const loaded = useLoad(() => projectSetupApi.sdkSetup({ params: { projectId } }), [projectId]);
  const project = useLoad(() => projectName(projectId), [projectId]);
  useSetNavProject(project.data);
  useTitle(PLATFORMS[platform]);

  const Page = PAGES[platform];
  return (
    <Load loaded={loaded}>
      {/* the count when the page was opened: what the polling compares with */}
      {(setup) => <Page projectId={projectId} dsn={setup.dsn} initialEventCount={setup.digested_event_count} />}
    </Load>
  );
}

/** `project_sdk_setup`: the DSN and how to use it, in general or per platform (`/sdk-setup/<platform>/`). */
export default function SdkSetup() {
  const projectId = useIntParam("projectId");
  const platform = useParams()["platform"] ?? "";
  if (projectId === null || !isPlatform(platform)) {
    return <NotFound />;
  }
  return <SdkSetupPage key={`${projectId}/${platform}`} projectId={projectId} platform={platform} />;
}
