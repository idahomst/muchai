// Ambient declarations for the whole app.

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
