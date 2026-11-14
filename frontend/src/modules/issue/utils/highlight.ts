/**
 * Upstream's `pygmentize` filter with highlight.js: the frame's lines highlighted together (language from the
 * filename, else from the event's platform), then split back into lines.
 */
import hljs from "highlight.js/lib/core";
import actionscript from "highlight.js/lib/languages/actionscript";
import bash from "highlight.js/lib/languages/bash";
import c from "highlight.js/lib/languages/c";
import clojure from "highlight.js/lib/languages/clojure";
import coffeescript from "highlight.js/lib/languages/coffeescript";
import cpp from "highlight.js/lib/languages/cpp";
import crystal from "highlight.js/lib/languages/crystal";
import csharp from "highlight.js/lib/languages/csharp";
import css from "highlight.js/lib/languages/css";
import dart from "highlight.js/lib/languages/dart";
import elixir from "highlight.js/lib/languages/elixir";
import erlang from "highlight.js/lib/languages/erlang";
import go from "highlight.js/lib/languages/go";
import groovy from "highlight.js/lib/languages/groovy";
import haskell from "highlight.js/lib/languages/haskell";
import java from "highlight.js/lib/languages/java";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import kotlin from "highlight.js/lib/languages/kotlin";
import lua from "highlight.js/lib/languages/lua";
import objectivec from "highlight.js/lib/languages/objectivec";
import perl from "highlight.js/lib/languages/perl";
import php from "highlight.js/lib/languages/php";
import powershell from "highlight.js/lib/languages/powershell";
import python from "highlight.js/lib/languages/python";
import ruby from "highlight.js/lib/languages/ruby";
import rust from "highlight.js/lib/languages/rust";
import scala from "highlight.js/lib/languages/scala";
import sql from "highlight.js/lib/languages/sql";
import swift from "highlight.js/lib/languages/swift";
import typescript from "highlight.js/lib/languages/typescript";
import xml from "highlight.js/lib/languages/xml";

const LANGUAGES = {
  actionscript,
  bash,
  c,
  clojure,
  coffeescript,
  cpp,
  crystal,
  csharp,
  css,
  dart,
  elixir,
  erlang,
  go,
  groovy,
  haskell,
  java,
  javascript,
  json,
  kotlin,
  lua,
  objectivec,
  perl,
  php,
  powershell,
  python,
  ruby,
  rust,
  scala,
  sql,
  swift,
  typescript,
  xml
};
type Language = keyof typeof LANGUAGES;

for (const [name, language] of Object.entries(LANGUAGES)) {
  hljs.registerLanguage(name, language);
}

const BY_EXTENSION: Record<string, Language> = {
  py: "python",
  pyw: "python",
  js: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  jsx: "javascript",
  ts: "typescript",
  tsx: "typescript",
  mts: "typescript",
  cts: "typescript",
  java: "java",
  kt: "kotlin",
  kts: "kotlin",
  php: "php",
  rb: "ruby",
  erb: "ruby",
  go: "go",
  cs: "csharp",
  c: "c",
  h: "c",
  cc: "cpp",
  cpp: "cpp",
  cxx: "cpp",
  hpp: "cpp",
  hh: "cpp",
  rs: "rust",
  swift: "swift",
  m: "objectivec",
  mm: "objectivec",
  scala: "scala",
  ex: "elixir",
  exs: "elixir",
  erl: "erlang",
  pl: "perl",
  pm: "perl",
  lua: "lua",
  dart: "dart",
  html: "xml",
  htm: "xml",
  xml: "xml",
  vue: "xml",
  svelte: "xml",
  css: "css",
  json: "json",
  sql: "sql",
  sh: "bash",
  bash: "bash",
  groovy: "groovy",
  gradle: "groovy",
  clj: "clojure",
  hs: "haskell",
  ps1: "powershell",
  cr: "crystal",
  coffee: "coffeescript",
  as: "actionscript"
};

/** `lexer_for_platform`; platforms without a lexer get none (pygments' `TextLexer`). */
const BY_PLATFORM: Record<string, Language> = {
  as3: "actionscript",
  c: "c",
  cfml: "xml",
  csharp: "csharp",
  crystal: "crystal",
  haskell: "haskell",
  go: "go",
  groovy: "groovy",
  java: "java",
  javascript: "javascript",
  node: "javascript",
  objc: "objectivec",
  perl: "perl",
  php: "php",
  powershell: "powershell",
  python: "python",
  ruby: "ruby"
};

function languageFor(filename: string | null, platform: string): Language | undefined {
  const basename = filename?.split(/[/\\]/).pop() ?? "";
  // query strings and the like, as in JavaScript URLs
  const extension = /\.([A-Za-z0-9]+)(?:[?#].*)?$/.exec(basename)?.[1]?.toLowerCase();
  return (extension ? BY_EXTENSION[extension] : undefined) ?? BY_PLATFORM[platform];
}

/** Splits highlighted HTML into lines, closing the spans open at each line end and reopening them on the next. */
function splitHighlighted(html: string): string[] {
  const lines: string[] = [];
  const open: string[] = [];
  let current = "";
  for (const token of html.split(/(<span[^>]*>|<\/span>|\n)/)) {
    if (token === "\n") {
      lines.push(current + "</span>".repeat(open.length));
      current = open.join("");
    } else if (token.startsWith("<span")) {
      open.push(token);
      current += token;
    } else if (token === "</span>") {
      open.pop();
      current += token;
    } else {
      current += token;
    }
  }
  lines.push(current);
  return lines;
}

/**
 * The lines as HTML (highlight.js escapes the code), or null when the language is unknown or highlighting doesn't
 * give back as many lines (then upstream shows them unformatted too).
 */
export function highlightLines(lines: string[], filename: string | null, platform: string): string[] | null {
  const language = languageFor(filename, platform);
  if (!language || lines.length === 0) {
    return null;
  }
  // newlines in a line happen (upstream's note: Django's TemplateSyntaxError); empty lines become a space
  const code = lines.map((line) => line.replace(/\n/g, "") || " ").join("\n");
  try {
    const result = splitHighlighted(hljs.highlight(code, { language, ignoreIllegals: true }).value);
    return result.length === lines.length ? result : null;
  } catch {
    return null;
  }
}
