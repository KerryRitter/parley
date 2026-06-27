import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// Vite config for the Parley desktop frontend. Tauri loads the dev server in
// development and the built `dist/` in production.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  // Don't pre-bundle the @cruzjs/ui barrel: it re-exports framework-coupled
  // components (Toast → @cruzjs/core, TabNavigation → react-router) that the dev
  // optimizer can't resolve. We only import via subpaths (see cruz.ts), so Vite
  // can transform those on demand.
  optimizeDeps: { exclude: ["@cruzjs/ui"] },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    outDir: "dist",
    target: "esnext",
    emptyOutDir: true,
  },
});
