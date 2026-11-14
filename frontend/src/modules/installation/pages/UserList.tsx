import { useState } from "react";
import { Link } from "react-router";

import Load from "common/components/Load";
import Messages from "common/components/Messages";
import { routes } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { useMessages } from "common/hooks/useMessages";
import { useTitle } from "common/hooks/useTitle";
import { useUser } from "contexts/session";
import { isStatus } from "services/http";
import { installationApi, type UserRow } from "services/installation";

const secondaryButton =
  "font-bold text-slate-500 dark:text-slate-300 border-slate-300 dark:border-slate-600 pl-4 pr-4 pb-2 pt-2 ml-2 border-2 hover:bg-slate-200 dark:hover:bg-slate-800 active:ring-3 rounded-md";
const dangerButton =
  "font-bold py-2 px-4 rounded-sm bg-red-500 dark:bg-red-700 text-white border-2 border-red-600 dark:border-red-400 hover:bg-red-600 dark:hover:bg-red-800 active:ring-3";

/** `user_list.js`'s delete confirmation modal. */
function DeleteModal({ onCancel, onConfirm }: { onCancel: () => void; onConfirm: () => void }) {
  return (
    <div className="fixed inset-0 bg-slate-600/50 dark:bg-slate-900/50 overflow-y-auto h-full w-full z-50 flex items-center justify-center">
      <div className="relative p-6 border border-slate-300 dark:border-slate-600 w-96 shadow-lg rounded-md bg-white dark:bg-slate-900">
        <div className="text-center m-4">
          <h3 className="text-2xl font-semibold text-slate-800 dark:text-slate-100 mt-3 mb-4">Delete User</h3>
          <div className="mt-4 mb-6">
            <p className="text-slate-700 dark:text-slate-300">
              Are you sure you want to delete this user? This action cannot be undone.
            </p>
          </div>
          <div className="flex items-center justify-center space-x-4 mb-4">
            <button type="button" className="text-cyan-500 dark:text-cyan-300 font-bold" onClick={onCancel}>
              Cancel
            </button>
            <button type="button" className={dangerButton} onClick={onConfirm}>
              Confirm
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

/** `user_list` (superusers): every user, to edit, (de)activate, or delete once inactive. */
export default function UserList() {
  useTitle("Users");
  const me = useUser();
  const loaded = useLoad(() => installationApi.users(), []);
  const { messages, add } = useMessages();
  const [deleting, setDeleting] = useState<UserRow | null>(null);

  async function setActive(user: UserRow, active: boolean) {
    const params = { params: { userId: user.id } };
    try {
      if (active) {
        await installationApi.activateUser(undefined, params);
        add(`User ${user.username} activated`);
      } else {
        await installationApi.deactivateUser(undefined, params);
        add(`User ${user.username} deactivated`);
      }
    } finally {
      loaded.reload();
    }
  }

  async function remove(user: UserRow) {
    setDeleting(null);
    try {
      await installationApi.deleteUser(undefined, { params: { userId: user.id } });
      add(`User ${user.username} deleted`);
    } catch (error) {
      if (!isStatus(error, 409)) {
        throw error;
      }
      add(`Cannot delete active user ${user.username}`);
    } finally {
      loaded.reload();
    }
  }

  return (
    <Load loaded={loaded}>
      {(users) => (
        <>
          {deleting && <DeleteModal onCancel={() => setDeleting(null)} onConfirm={() => void remove(deleting)} />}

          <div className="flex items-center justify-center">
            <div className="m-4 max-w-4xl flex-auto">
              <Messages messages={messages} className="mb-4" />

              <div className="flex">
                <h1 className="text-4xl mt-4 font-bold">Users</h1>
              </div>

              <div>
                <table className="w-full mt-8">
                  <thead>
                    <tr className="bg-slate-200 dark:bg-slate-800">
                      <th className="w-full p-4 text-left text-xl" colSpan={2}>
                        Users
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {users.map((user) => (
                      <tr
                        key={user.id}
                        className="bg-white dark:bg-slate-900 border-slate-200 dark:border-slate-700 border-b-2"
                      >
                        <td className="w-full p-4">
                          <div>
                            <Link
                              to={routes.userEdit(user.id)}
                              className="text-xl text-cyan-500 dark:text-cyan-300 font-bold"
                            >
                              {user.username}
                            </Link>
                          </div>
                        </td>

                        <td className="p-4">
                          <div className="flex justify-end">
                            {user.id !== me.id &&
                              (user.is_active ? (
                                <button
                                  type="button"
                                  className={secondaryButton}
                                  onClick={() => void setActive(user, false)}
                                >
                                  Deactivate
                                </button>
                              ) : (
                                <>
                                  <button
                                    type="button"
                                    className={secondaryButton}
                                    onClick={() => void setActive(user, true)}
                                  >
                                    Activate
                                  </button>
                                  <button
                                    type="button"
                                    className={`${dangerButton} ml-4`}
                                    onClick={() => setDeleting(user)}
                                  >
                                    Delete
                                  </button>
                                </>
                              ))}
                          </div>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          </div>
        </>
      )}
    </Load>
  );
}
