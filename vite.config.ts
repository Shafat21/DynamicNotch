import { defineConfig } from "vite";

// Tauri expects a fixed port and no screen clearing so Rust errors stay visible.
export default defineConfig({
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**"] } },
  build: {
    target: "chrome120",
    outDir: "dist",
    emptyOutDir: true,
    // island + Settings window (created on demand by `open_settings`) + the screenshot overlay (`capture_start`)
    rollupOptions: { input: { main: "index.html", settings: "settings.html", capture: "capture.html" } },
  },
});
