import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri-friendly Vite config. HMR is disabled because WebKit (the engine
// Tauri uses on macOS) tends to hang on localhost WebSocket connections,
// which blocks the initial page load. Full ⌘+R reload still works.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    host: "127.0.0.1",
    hmr: false,
    watch: { ignored: ["**/src-tauri/**"] },
  },
});
