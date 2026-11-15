/** Upstream's `project.get_warnings` banners (its ingest quota), atop its issue list and its issues' pages. */
export default function ProjectWarnings({ warnings }: { warnings: string[] }) {
  return warnings.map((warning) => (
    <div
      key={warning}
      className="flex p-4 bg-yellow-100 dark:bg-amber-800 border-b-2 border-yellow-200 dark:border-amber-900"
    >
      <div className="text-slate-800 dark:text-slate-100">{warning}</div>
    </div>
  ));
}
