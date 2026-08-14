// Ambient declarations for the whole app.

// Vite's own client types — `?raw` imports and `import.meta.glob`, both used by
// src/lib/guides.ts. Nothing else in the project pulls these in, and
// .svelte-kit/tsconfig.json sets `types: none`, so without this line
// svelte-check rejects the guide imports. A triple-slash reference is not an
// import, so this file stays a global script (see the note below).
/// <reference types="vite/client" />

/**
 * The version from package.json, supplied by the `define` block in
 * vite.config.js — a real global in dev, statically replaced in a build.
 *
 * A component must NOT import package.json directly instead: the project root
 * is outside SvelteKit's server.fs.allow list, so `tauri dev` refuses to serve
 * it and the app fails to boot.
 */
// No import/export in this file — that would turn it into a module and the
// declaration below would stop being global.
declare const __APP_VERSION__: string;
