import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// The backend (`bugsink serve`, default port 8000) serves the API; in development this server proxies to it. The
// proxy keeps the browser's Host header, so the backend's CSRF origin check sees a same-origin request.
const BACKEND = process.env["BUGSINK_BACKEND"] ?? "http://127.0.0.1:8000";

export default defineConfig({
  resolve: {
    tsconfigPaths: true
  },
  plugins: [tailwindcss(), react()],
  server: {
    port: 3000,
    proxy: {
      "/api": { target: BACKEND },
      "/health": { target: BACKEND }
    }
  }
});
