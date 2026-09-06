import { defineConfig } from "vite";

// Tauri expects a fixed dev port and wants the config served without
// clearing the terminal.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    target: "es2022",
    outDir: "dist",
    emptyOutDir: true,
  },
});
