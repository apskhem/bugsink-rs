import { useEffect, useState } from "react";

/** Upstream's `copy.js`: the clipboard API where available (secure contexts), else `execCommand("copy")`. */
async function copyText(text: string) {
  if (navigator.clipboard) {
    return navigator.clipboard.writeText(text);
  }
  const textArea = document.createElement("textarea");
  textArea.value = text;
  textArea.style.position = "fixed";
  textArea.style.top = "0";
  textArea.style.left = "0";
  textArea.style.opacity = "0";
  document.body.appendChild(textArea);
  textArea.focus();
  textArea.select();
  const copied = document.execCommand("copy");
  document.body.removeChild(textArea);
  if (!copied) {
    throw new Error("copy failed");
  }
}

function useCopied(): [boolean, (text: string) => void] {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) {
      return undefined;
    }
    const timeout = setTimeout(() => setCopied(false), 2500);
    return () => clearTimeout(timeout);
  }, [copied]);
  return [copied, (text) => void copyText(text.trim()).then(() => setCopied(true))];
}

function CopyIcon({ className }: { className: string }) {
  return (
    <svg className={className} viewBox="0 0 20 20" fill="currentColor" aria-hidden="true">
      <path d="M7 2a2 2 0 0 0-2 2v2h2V4h8v8h-2v2h2a2 2 0 0 0 2-2V4a2 2 0 0 0-2-2H7Z" />
      <path d="M3 8a2 2 0 0 1 2-2h6a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8Zm2 0v8h6V8H5Z" />
    </svg>
  );
}

function CopiedIcon({ className }: { className: string }) {
  return (
    <svg className={className} viewBox="0 0 20 20" fill="currentColor" aria-hidden="true">
      <path
        fillRule="evenodd"
        d="M16.7 5.3a1 1 0 0 1 0 1.4l-7.5 7.5a1 1 0 0 1-1.4 0l-3.5-3.5a1 1 0 1 1 1.4-1.4l2.8 2.8 6.8-6.8a1 1 0 0 1 1.4 0Z"
        clipRule="evenodd"
      />
    </svg>
  );
}

/** Upstream's `includes/copy_button_only.html`: a button, or (`link`) a small icon. */
export function CopyButtonOnly({ value, link, secondary }: { value: string; link?: boolean; secondary?: boolean }) {
  const [copied, copy] = useCopied();
  if (link) {
    return (
      <a
        href="#"
        aria-label="Copy"
        title="Copy"
        className="inline-flex items-center text-slate-500 dark:text-slate-400 hover:text-slate-700 dark:hover:text-slate-300"
        onClick={(event) => {
          event.preventDefault();
          copy(value);
        }}
      >
        <span className="sr-only" aria-live="polite">
          {copied ? "Copied!" : "Copy"}
        </span>
        {copied ? <CopiedIcon className="w-4 h-4" /> : <CopyIcon className="w-4 h-4" />}
      </a>
    );
  }
  const colors = secondary
    ? "text-slate-500 dark:text-slate-300 border-slate-300 dark:border-slate-600 hover:bg-slate-200 dark:hover:bg-slate-800"
    : "text-slate-800 dark:text-slate-100 border-slate-500 dark:border-slate-400 bg-cyan-200 dark:bg-cyan-700 hover:bg-cyan-400 dark:hover:bg-cyan-600";
  return (
    <button
      type="button"
      className={`inline-flex items-center gap-2 font-bold border-2 active:ring-3 rounded-md px-3 py-2 ${colors}`}
      onClick={() => copy(value)}
    >
      {copied ? <CopiedIcon className="w-5 h-5" /> : <CopyIcon className="w-5 h-5" />}
      <span>{copied ? "Copied!" : "Copy"}</span>
    </button>
  );
}

/** Upstream's `includes/copy_button.html`: the value in a code box, with the button. */
export function CopyButton({ value }: { value: string }) {
  return (
    <div className="flex items-center gap-2 mt-2">
      <code className="flex-1 min-w-0 overflow-x-auto whitespace-nowrap bg-slate-100 dark:bg-slate-800 border-2 border-slate-300 dark:border-slate-600 rounded-md px-3 py-2 text-sm">
        {value}
      </code>
      <CopyButtonOnly value={value} />
    </div>
  );
}
