import { useEffect } from "react";
import { useNavigate } from "react-router";

import { LoadError } from "common/components/ErrorPage";
import { routes } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { sessionApi } from "services/session";

/**
 * Upstream's `home`: to the issues of your one project; else, with any project, to the project list; else to the team
 * list (the project list in single-team mode).
 */
export default function Home() {
  const navigate = useNavigate();
  const loaded = useLoad(() => sessionApi.home(), []);
  const home = loaded.data;

  useEffect(() => {
    if (!home) {
      return;
    }
    switch (home.go) {
      case "issues":
        void navigate(routes.issues(home.project_id), { replace: true });
        break;
      case "projects":
        void navigate(routes.projects(), { replace: true });
        break;
      case "teams":
        void navigate(routes.teams(), { replace: true });
        break;
    }
  }, [home, navigate]);

  return loaded.state === "failed" ? <LoadError error={loaded.error} /> : null;
}
