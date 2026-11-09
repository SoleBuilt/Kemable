import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

// Tauri loads the dev server at a fixed port (src-tauri/tauri.conf.json, build.devUrl).
export default defineConfig({
  plugins: [svelte(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    // WebView2 is evergreen Chromium, so no syntax needs lowering.
    target: "esnext",
    sourcemap: false,
    // Loaded from disk, not the network: one bundle is fine.
    chunkSizeWarningLimit: 1500,
  },
});
