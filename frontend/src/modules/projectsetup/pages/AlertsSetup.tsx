import { Link } from "react-router";

import { NotFound } from "common/components/ErrorPage";
import Load from "common/components/Load";
import Messages from "common/components/Messages";
import { routes } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { useMessages } from "common/hooks/useMessages";
import { useTitle } from "common/hooks/useTitle";
import { useSetNavProject } from "common/layouts/NavProject";
import WarningIcon from "modules/projectsetup/components/WarningIcon";
import { useIntParam } from "modules/projectsetup/hooks/useIntParam";
import { formatDate } from "modules/projectsetup/utils/messagingServices";
import { projectsApi } from "services/projects";
import { projectSetupApi } from "services/projectsetup";

const secondaryButton =
  "font-bold text-slate-500 dark:text-slate-300 border-slate-300 dark:border-slate-600 pl-4 pr-4 pb-2 pt-2 ml-2 border-2 hover:bg-slate-200 dark:hover:bg-slate-800 active:ring-3 rounded-md whitespace-nowrap";

function AlertsSetupPage({ projectId }: { projectId: number }) {
  const loaded = useLoad(
    () =>
      Promise.all([
        projectsApi.projectSummary({ params: { projectId } }),
        projectSetupApi.messagingServices({ params: { projectId } })
      ]),
    [projectId]
  );
  const project = loaded.data?.[0];
  useSetNavProject(project);
  useTitle(["Alerts", project?.name]);
  const { messages, add } = useMessages();

  async function test(serviceId: number) {
    await projectSetupApi.testMessagingService(undefined, { params: { projectId, serviceId } });
    add("Test message sent; check the configured service to see if it arrived.");
  }

  async function remove(serviceId: number) {
    try {
      await projectSetupApi.removeMessagingService(undefined, { params: { projectId, serviceId } });
    } finally {
      loaded.reload();
    }
  }

  return (
    <Load loaded={loaded}>
      {([project, { services }]) => (
        <div className="flex items-center justify-center">
          <div className="m-4 max-w-4xl flex-auto">
            <Messages messages={messages} className="mb-4" />

            <div className="flex">
              <h1 className="text-4xl mt-4 font-bold">Alerts · {project.name}</h1>

              <div className="ml-auto mt-6">
                <Link
                  className="font-bold text-slate-800 dark:text-slate-100 border-slate-500 dark:border-slate-400 pl-4 pr-4 pb-2 pt-2 ml-1 border-2 bg-cyan-200 dark:bg-cyan-700 hover:bg-cyan-400 dark:hover:bg-cyan-600 active:ring-3 rounded-md"
                  to={routes.projectMessagingServiceAdd(projectId)}
                >
                  Add
                </Link>
              </div>
            </div>

            <div>
              <table className="w-full mt-8">
                <thead>
                  <tr className="bg-slate-200 dark:bg-slate-800">
                    <th className="w-full p-4 text-left text-xl" colSpan={2}>
                      Messaging Services
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {services.map((service) => (
                    <tr
                      key={service.id}
                      className="bg-white dark:bg-slate-900 border-slate-200 dark:border-slate-700 border-b-2"
                    >
                      <td className="w-full p-4">
                        <div className="flex items-center">
                          <div className="flex-grow">
                            <Link
                              to={routes.projectMessagingServiceEdit(projectId, service.id)}
                              className="text-xl text-cyan-500 dark:text-cyan-300 font-bold"
                            >
                              {service.display_name}
                            </Link>
                            {service.last_failure_timestamp !== null && (
                              <div className="text-sm text-red-600 dark:text-red-400 mt-1">
                                <span className="inline-flex items-center leading-none">
                                  <WarningIcon className="w-4 h-4 mr-1 align-middle" />
                                  <span>
                                    Last alert failed ({formatDate(service.last_failure_timestamp, "MMM D, HH:mm")})
                                  </span>
                                  <Link
                                    to={`${routes.projectMessagingServiceEdit(projectId, service.id)}#failure-details`}
                                    className="text-red-700 dark:text-red-300 underline ml-2"
                                  >
                                    View details
                                  </Link>
                                </span>
                              </div>
                            )}
                          </div>
                        </div>
                      </td>

                      <td className="p-4">
                        <div className="flex justify-end">
                          <button type="button" className={secondaryButton} onClick={() => void test(service.id)}>
                            Test
                          </button>
                          <button type="button" className={secondaryButton} onClick={() => void remove(service.id)}>
                            Remove
                          </button>
                        </div>
                      </td>
                    </tr>
                  ))}
                  {!services.length && (
                    <tr className="bg-white dark:bg-slate-900 border-slate-200 dark:border-slate-700 border-b-2">
                      <td className="w-full p-4">
                        <div>
                          No Messaging Services Configured.{" "}
                          <Link
                            to={routes.projectMessagingServiceAdd(projectId)}
                            className="text-cyan-500 dark:text-cyan-300 font-bold"
                          >
                            Add Messaging Service
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
              <div className="ml-auto py-8 pr-4">
                <Link to={routes.projectEdit(projectId)} className="text-cyan-500 dark:text-cyan-300 font-bold">
                  Settings
                </Link>{" "}
                <span className="font-bold text-slate-500 dark:text-slate-300">|</span>{" "}
                <Link to={routes.projects()} className="text-cyan-500 dark:text-cyan-300 font-bold">
                  Back to Projects
                </Link>
              </div>
            </div>
          </div>
        </div>
      )}
    </Load>
  );
}

/** `project_alerts_setup`: the project's messaging services, to test, edit or remove, and to add more. */
export default function AlertsSetup() {
  const projectId = useIntParam("projectId");
  return projectId === null ? <NotFound /> : <AlertsSetupPage key={projectId} projectId={projectId} />;
}
