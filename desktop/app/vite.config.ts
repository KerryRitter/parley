import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// Vite config for the Parley desktop frontend. Tauri loads the dev server in
// development and the built `dist/` in production.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  // @cruzjs/ui ships TypeScript source; let Vite prebundle it.
  optimizeDeps: { include: ["@cruzjs/ui"] },
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
