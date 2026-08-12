import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";
import { version } from "./package.json";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [sveltekit()],

  // The app version is baked in here rather than imported by a component.
  // SvelteKit builds Vite's server.fs.allow list out of src/, .svelte-kit/ and
  // node_modules/ — the project root is NOT on it, so importing package.json
  // from the client 404s under `tauri dev`. A build inlines the import instead
  // of serving it, which is why this only ever bit the dev server.
  // Declared for svelte-check in src/app.d.ts.
  define: {
    __APP_VERSION__: JSON.stringify(version),
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    // Bind loopback only (never the LAN) unless TAURI_DEV_HOST is explicitly set
    // for mobile/remote dev. This is a dev-only server; `tauri build` bundles the
    // frontend and opens no port.
    host: host || "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
