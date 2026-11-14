import { useState } from "react";
import { Link } from "react-router";

import { CopyButtonOnly } from "common/components/CopyButton";
import Load from "common/components/Load";
import Messages from "common/components/Messages";
import { routes } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { useMessages } from "common/hooks/useMessages";
import { useTitle } from "common/hooks/useTitle";
import { installationApi, type AuthToken } from "services/installation";

/** The token's capabilities by prefix (`issues: (read, comment)`), in upstream's order. */
function capabilityGroups(capabilities: string[]): [string, string[]][] {
  const groups = new Map<string, string[]>();
  for (const capability of capabilities) {
    const colon = capability.indexOf(":");
    const prefix = capability.slice(0, colon);
    const name = capability.slice(colon + 1);
    groups.set(prefix, [...(groups.get(prefix) ?? []), name]);
  }
  return [...groups];
}

function TokenRow({ authToken, onRevoke }: { authToken: AuthToken; onRevoke: () => void }) {
  const [revealed, setRevealed] = useState(false);
  const groups = capabilityGroups(authToken.capabilities);
  return (
    <tr className="bg-white dark:bg-slate-900 border-slate-200 dark:border-slate-700 border-b-2">
      <td className="w-full p-4">
        <div className="flex items-center gap-2">
          <span className={`font-mono${revealed ? " hidden" : ""}`} aria-hidden="true">
            ••••••••••••••••••••••••••••••••••••••••
          </span>
          <span id={`token-revealed-${authToken.id}`} className={`font-mono${revealed ? "" : " hidden"}`}>
            {authToken.token}
          </span>
          <button
            type="button"
            onClick={() => setRevealed(!revealed)}
            aria-expanded={revealed}
            aria-controls={`token-revealed-${authToken.id}`}
            className="text-slate-500 dark:text-slate-400 hover:text-slate-700 dark:hover:text-slate-300 text-sm underline"
          >
            {revealed ? "Hide" : "Reveal"}
          </button>
          <CopyButtonOnly value={authToken.token} link />
        </div>
        <div className="mt-2 font-bold text-slate-800 dark:text-slate-100">{authToken.description}</div>
        <div className="mt-1 text-slate-600 dark:text-slate-400 text-sm">
          {authToken.is_user_bound
            ? `Personal token for ${authToken.user ? authToken.user.username : "deleted user"}`
            : "Service token"}{" "}
          ·{" "}
          {authToken.is_project_bound
            ? (authToken.project?.name ?? "")
            : authToken.is_user_bound
              ? "All my projects"
              : "All projects"}
        </div>
        <div className="mt-2 text-slate-600 dark:text-slate-400 text-sm">
          {authToken.has_all_capabilities ? (
            <span className="font-bold">All capabilities</span>
          ) : groups.length ? (
            <div className="grid grid-cols-[7rem_minmax(0,1fr)] gap-x-2">
              {groups.map(([prefix, names]) => (
                <div key={prefix} className="contents">
                  <code className="text-right">{prefix}:</code>
                  <code>({names.join(", ")})</code>
                </div>
              ))}
            </div>
          ) : (
            "No capabilities"
          )}
        </div>
      </td>

      <td className="p-4">
        <div className="flex justify-end">
          <button
            type="button"
            onClick={onRevoke}
            className="font-bold text-slate-500 dark:text-slate-300 border-slate-300 dark:border-slate-600 pl-4 pr-4 pb-2 pt-2 ml-2 border-2 hover:bg-slate-200 dark:hover:bg-slate-800 active:ring-3 rounded-md whitespace-nowrap"
          >
            Revoke
          </button>
        </div>
      </td>
    </tr>
  );
}

/** `auth_token_list`: the tokens for the REST API (all of them for superusers, else your own). */
export default function AuthTokenList() {
  useTitle("Auth Tokens");
  const loaded = useLoad(() => installationApi.authTokens(), []);
  const { messages, add } = useMessages();

  async function revoke(tokenId: number) {
    try {
      await installationApi.revokeAuthToken(undefined, { params: { tokenId } });
      add("Token revoked");
    } finally {
      loaded.reload();
    }
  }

  return (
    <Load loaded={loaded}>
      {(authTokens) => (
        <div className="flex items-center justify-center">
          <div className="m-4 max-w-4xl flex-auto">
            <Messages messages={messages} className="mb-4" />

            <div className="flex">
              <h1 className="text-4xl mt-4 font-bold">Auth Tokens</h1>

              <div className="ml-auto mt-6">
                <Link
                  to={routes.authTokenCreate()}
                  className="font-bold text-slate-800 dark:text-slate-100 border-slate-500 dark:border-slate-400 pl-4 pr-4 pb-2 pt-2 ml-1 border-2 bg-cyan-200 dark:bg-cyan-700 hover:bg-cyan-400 dark:hover:bg-cyan-600 active:ring-3 rounded-md"
                >
                  Add Token
                </Link>
              </div>
            </div>

            <div>
              <table className="w-full mt-8">
                <thead>
                  <tr className="bg-slate-200 dark:bg-slate-800">
                    <th className="w-full p-4 text-left text-xl" colSpan={2}>
                      Auth Tokens
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {authTokens.map((authToken) => (
                    <TokenRow key={authToken.id} authToken={authToken} onRevoke={() => void revoke(authToken.id)} />
                  ))}
                  {!authTokens.length && (
                    <tr className="bg-white dark:bg-slate-900 border-slate-200 dark:border-slate-700 border-b-2">
                      <td className="w-full p-4">
                        <div>No Auth Tokens.</div>
                      </td>
                      <td className="p-4"></td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          </div>
        </div>
      )}
    </Load>
  );
}
