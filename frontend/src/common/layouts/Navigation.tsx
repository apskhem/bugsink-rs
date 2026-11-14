import { Link, useLocation, useNavigate } from "react-router";

import { routes } from "common/constants/routes";
import { useSession } from "contexts/session";
import { sessionApi } from "services/session";

import { useNavProject } from "./NavProject";

const item = "px-4 py-2 my-2 hover:bg-slate-300 dark:hover:bg-slate-700 rounded-xl";

/** Upstream's `theme/navigation.html`: a push-down menu on mobile, a bar on desktop. */
export default function Navigation() {
  const { user, settings, setUser } = useSession();
  const project = useNavProject();
  const navigate = useNavigate();
  const { search } = useLocation();

  async function logout() {
    await sessionApi.logout(undefined);
    setUser(null);
    await navigate(routes.logout());
  }

  // upstream keeps the query string (`current_qs`) on the project's issue list link
  const projectIssues = project ? `${routes.issues(project.id)}${search}` : null;

  return (
    <>
      <header className="w-full bg-slate-200 dark:bg-slate-800 md:hidden">
        <input type="checkbox" id="nav-toggle" className="peer hidden" />
        <nav className="flex items-center justify-between px-4 py-2">
          <Link to={routes.home()} className="font-bold">
            {settings.site_title}
          </Link>
          <label htmlFor="nav-toggle" className="cursor-pointer">
            <svg
              className="w-6 h-6"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
            >
              <path d="M4 6h16M4 12h16M4 18h16" />
            </svg>
          </label>
        </nav>
        <div className="hidden peer-checked:block px-4 pb-4">
          <div className="flex flex-col gap-3 text-right">
            {!settings.single_team && <Link to={routes.teams()}>Teams</Link>}
            <Link to={routes.projects()}>Projects</Link>
            <Link to={routes.globalIssues()}>Issues</Link>
            {project && projectIssues && <Link to={projectIssues}>Issues ({project.name})</Link>}
            <Link to={routes.preferences()}>Preferences</Link>
            <button type="button" className="text-right w-full" onClick={() => void logout()}>
              Log out
            </button>
          </div>
        </div>
      </header>

      <header className="hidden md:flex pl-4 bg-slate-200 dark:bg-slate-800 items-center">
        <Link to={routes.home()}>
          <img src="/images/bugsink-logo.png" className="p-2 h-12 w-12 dark:hidden block" alt="Bugsink logo" />
          <img src="/images/bugsink-logo-dark.png" className="p-2 h-12 w-12 hidden dark:block" alt="Bugsink logo" />
        </Link>
        <Link to={routes.home()}>
          <div className="px-2 py-2 my-2 font-bold hover:bg-slate-300 dark:hover:bg-slate-700 rounded-xl">
            {settings.site_title}
          </div>
        </Link>

        {!settings.single_team && (
          <Link to={routes.teams()}>
            <div className={item}>Teams</div>
          </Link>
        )}
        <Link to={routes.projects()}>
          <div className={item}>Projects</div>
        </Link>
        <Link to={routes.globalIssues()}>
          <div className={item}>Issues</div>
        </Link>
        {project && projectIssues && (
          <Link to={projectIssues}>
            <div className={item}>Issues ({project.name})</div>
          </Link>
        )}

        <div className="ml-auto flex">
          {user?.is_superuser && (
            <Link to={routes.users()}>
              <div className={item}>Users</div>
            </Link>
          )}
          {user ? (
            <>
              <Link to={routes.authTokens()}>
                <div className={item}>Tokens</div>
              </Link>
              {/* served by the backend, not a client route */}
              <a href={routes.openApi()}>
                <div className={item}>OpenAPI</div>
              </a>
              <Link to={routes.preferences()}>
                <div className={item}>Preferences</div>
              </Link>
              <div className={`${item} mr-2`}>
                <button type="button" onClick={() => void logout()}>
                  Log out
                </button>
              </div>
            </>
          ) : (
            <Link to={routes.login()}>
              <div className={item}>Login</div>
            </Link>
          )}
        </div>
      </header>
    </>
  );
}
