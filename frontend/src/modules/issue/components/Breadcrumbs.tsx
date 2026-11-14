import type { ReactNode } from "react";

import { TimestampWithMillis } from "modules/issue/components/Text";
import { asRecord, djStr, truthy } from "modules/issue/utils/python";

type Props = { breadcrumbs: unknown[] | null; eventLine: ReactNode; nav: ReactNode };

/** Upstream's `issues/breadcrumbs.html`. */
export default function Breadcrumbs({ breadcrumbs, eventLine, nav }: Props) {
  return (
    <>
      <div className="flex">
        <div className="overflow-hidden">
          <div className="italic text-ellipsis whitespace-nowrap overflow-hidden">{eventLine}</div>
        </div>
        <div className="ml-auto flex-none">
          <div className="flex place-content-end">{nav}</div>
        </div>
      </div>

      {!truthy(breadcrumbs) ? (
        <div className="mt-6 mb-6 italic">No breadcrumbs available for this event.</div>
      ) : (
        <div className="pt-4">
          <table className="w-full">
            <tbody>
              {(breadcrumbs ?? []).map((value, i) => {
                const breadcrumb = asRecord(value) ?? {};
                return (
                  <tr key={i} className="border-slate-200 dark:border-slate-700 border-2 ">
                    <td className="p-4 font-bold text-slate-500 dark:text-slate-300 align-top">
                      {djStr(breadcrumb["category"])}
                    </td>
                    <td className="w-full p-4 font-mono">{djStr(breadcrumb["message"])}</td>
                    <td className="p-4 font-bold text-slate-500 dark:text-slate-300 align-top">
                      <TimestampWithMillis value={breadcrumb["timestamp"]} />
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}
