import hljs from "highlight.js/lib/core";
import go from "highlight.js/lib/languages/go";
import javascript from "highlight.js/lib/languages/javascript";
import php from "highlight.js/lib/languages/php";
import python from "highlight.js/lib/languages/python";

hljs.registerLanguage("go", go);
hljs.registerLanguage("javascript", javascript);
hljs.registerLanguage("php", php);
hljs.registerLanguage("python", python);

export type CodeLanguage = "text" | "go" | "javascript" | "php" | "python";

/**
 * Upstream's `{% code %}` tag: the code (trailing whitespace stripped, as Pygments' `stripall`) highlighted in a
 * `syntax-coloring` block.
 */
export default function CodeBlock({ language, children }: { language: CodeLanguage; children: string }) {
  const code = children
    .split("\n")
    .map((line) => line.trimEnd())
    .join("\n")
    .trim();
  return (
    <div className="p-4 mt-4 bg-slate-50 dark:bg-slate-800 syntax-coloring">
      {language === "text" ? (
        <pre>{code}</pre>
      ) : (
        // highlight.js escapes the code; only its own spans are markup (colored by styles/syntax.css)
        <pre dangerouslySetInnerHTML={{ __html: hljs.highlight(code, { language }).value }} />
      )}
    </div>
  );
}
