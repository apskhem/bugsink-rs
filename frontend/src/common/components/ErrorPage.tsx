import { apiError } from "services/http";

/** Upstream's `404.html` / `403.html` / `500.html` content. */
export function NotFound({ message }: { message?: string }) {
  return (
    <div className="m-4">
      <h1 className="text-4xl mt-4 font-bold">Not Found</h1>
      <div className="pt-2">{message}</div>
    </div>
  );
}

export function Forbidden({ message }: { message?: string }) {
  return (
    <div className="m-4">
      <h1 className="text-4xl mt-4 font-bold">Permission denied</h1>
      <div className="pt-2">{message}</div>
    </div>
  );
}

export function ServerError({ message }: { message?: string }) {
  return (
    <div className="m-4">
      <h1 className="text-4xl mt-4 font-bold">500 Server Error</h1>
      <div className="pt-2 font-bold">{message}</div>
      <div className="pt-6">
        You will find more information in:
        <ul className="list-disc pt-4 pl-4">
          <li>the server logs (stdout of your Docker, or in journalctl)</li>
          <li>
            if you're{" "}
            <a href="https://www.bugsink.com/docs/dogfooding/" className="text-cyan-500 dark:text-cyan-300 font-bold">
              dogfooding Bugsink
            </a>
            , in Bugsink itself
          </li>
        </ul>
      </div>
    </div>
  );
}

/** The page for a failed load: 404 and 403 as upstream shows them, anything else as a server error. */
export function LoadError({ error }: { error: unknown }) {
  const failed = apiError(error);
  if (failed?.status === 404) {
    return <NotFound />;
  }
  if (failed?.status === 403) {
    return <Forbidden message={failed.detail} />;
  }
  return <ServerError message={failed?.detail ?? String(error)} />;
}
