import { useState } from "react";
import { useSearchParams } from "react-router";

/**
 * Upstream's `theme/_search_input.html`: `?q=`, submitted with Enter; the × clears it (and the page). Other query
 * parameters are kept.
 */
export default function SearchInput({ placeholder, containerClass }: { placeholder: string; containerClass?: string }) {
  const [searchParams, setSearchParams] = useSearchParams();
  const q = searchParams.get("q") ?? "";
  const [value, setValue] = useState(q);

  function search(next: string) {
    const params = new URLSearchParams(searchParams);
    params.delete("page");
    if (next) {
      params.set("q", next);
    } else {
      params.delete("q");
    }
    setSearchParams(params);
  }

  return (
    <form
      className={`relative inline-block${containerClass ? ` ${containerClass}` : ""}`}
      onSubmit={(event) => {
        event.preventDefault();
        search(value);
      }}
    >
      <input
        type="text"
        name="q"
        value={value}
        placeholder={placeholder}
        onChange={(event) => setValue(event.target.value)}
        className="bg-slate-50 dark:bg-slate-800 focus:border-cyan-500 dark:focus:border-cyan-400 focus:ring-cyan-200 dark:focus:ring-cyan-700 rounded-md pr-9"
      />
      {q && (
        <a
          href="#"
          aria-label="Clear search"
          title="Clear search"
          className="absolute inset-y-0 right-0 flex items-center px-3 text-xl leading-none text-slate-400 hover:text-slate-700 dark:hover:text-slate-300"
          onClick={(event) => {
            event.preventDefault();
            setValue("");
            search("");
          }}
        >
          &times;
        </a>
      )}
    </form>
  );
}
