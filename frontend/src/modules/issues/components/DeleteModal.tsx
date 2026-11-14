/** Upstream's `#deleteModal`: confirms deleting the selected issues. */
export default function DeleteModal({ onCancel, onConfirm }: { onCancel: () => void; onConfirm: () => void }) {
  return (
    <div className="fixed inset-0 bg-slate-600/50 dark:bg-slate-900/50 overflow-y-auto h-full w-full z-50 flex items-center justify-center">
      <div className="relative p-6 border border-slate-300 dark:border-slate-600 w-96 shadow-lg rounded-md bg-white dark:bg-slate-900">
        <div className="text-center m-4">
          <h3 className="text-2xl font-semibold text-slate-800 dark:text-slate-100 mt-3 mb-4">Delete Issues</h3>
          <div className="mt-4 mb-6">
            <p className="text-slate-700 dark:text-slate-300">
              Deleting an Issue is a permanent action and cannot be undone. It's typically better to resolve or mute an
              issue instead of deleting it, as this allows you to keep track of past issues and their resolutions.
            </p>
          </div>
          <div className="flex items-center justify-center space-x-4 mb-4">
            <button type="button" className="text-cyan-500 dark:text-cyan-300 font-bold" onClick={onCancel}>
              Cancel
            </button>
            <button
              type="button"
              className="font-bold py-2 px-4 rounded-sm bg-red-500 dark:bg-red-700 text-white border-2 border-red-600 dark:border-red-400 hover:bg-red-600 dark:hover:bg-red-800 active:ring-3"
              onClick={onConfirm}
            >
              Delete
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
