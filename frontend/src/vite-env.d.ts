/// <reference types="vite/client" />

interface Window {
  /** Re-applies `data-theme-preference` (index.html's theme script). */
  __applyTheme?: () => void;
}
