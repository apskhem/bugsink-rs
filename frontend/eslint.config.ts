import tsParser from "@typescript-eslint/parser";
import importPlugin from "eslint-plugin-import";
import { defineConfig } from "eslint/config";

export default defineConfig({
  languageOptions: {
    parser: tsParser,
    parserOptions: {
      sourceType: "module"
    }
  },
  files: ["src/**/*.{ts,tsx}"],
  ignores: ["node_modules", "dist"],
  plugins: {
    import: importPlugin
  },
  rules: {
    "import/order": [
      "warn",
      {
        "newlines-between": "always",
        alphabetize: {
          order: "asc",
          caseInsensitive: false
        },
        pathGroups: [
          { pattern: "common/**", group: "parent" },
          { pattern: "contexts/**", group: "parent" },
          { pattern: "core/**", group: "parent" },
          { pattern: "modules/**", group: "parent" },
          { pattern: "services/**", group: "parent" },
          { pattern: "styles/**", group: "parent" },
          { pattern: "../**", group: "sibling" },
          { pattern: "./*.css", group: "sibling", position: "after" }
        ],
        pathGroupsExcludedImportTypes: []
      }
    ]
  }
});
