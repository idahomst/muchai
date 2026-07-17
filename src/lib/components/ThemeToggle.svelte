<script lang="ts">
  import { settings } from "$lib/stores";
  import { setSettings } from "$lib/api";
  import { applyTheme } from "$lib/theme";

  // `labeled` renders a self-explanatory button ("☀ Switch to light theme") for
  // the Preferences dialog; the default is the compact icon used in the header.
  let { labeled = false }: { labeled?: boolean } = $props();

  let busy = $state(false);

  // A not-yet-loaded config is treated as dark.
  const theme = $derived($settings?.theme ?? "dark");

  async function toggle() {
    if (!$settings || busy) return;
    const prev = $settings;
    const nextTheme: "dark" | "light" = prev.theme === "light" ? "dark" : "light";
    const next = { ...prev, theme: nextTheme };
    applyTheme(nextTheme); // instant visual swap + cache
    settings.set(next);    // optimistic
    busy = true;
    try {
      await setSettings(next);
    } catch {
      settings.set(prev);     // revert the store…
      applyTheme(prev.theme); // …and the visual theme
    } finally {
      busy = false;
    }
  }
</script>

<button
  class="theme-toggle"
  class:labeled
  onclick={toggle}
  disabled={busy}
  aria-label={theme === "light" ? "Switch to dark theme" : "Switch to light theme"}
  title={theme === "light" ? "Switch to dark theme" : "Switch to light theme"}>
  {#if labeled}
    {theme === "light" ? "☽ Switch to dark theme" : "☀ Switch to light theme"}
  {:else}
    {theme === "light" ? "☽" : "☀"}
  {/if}
</button>

<style>
  .theme-toggle {
    background:none; border:none; color:inherit; cursor:pointer;
    font-size:1rem; line-height:1; padding:.2rem .4rem; border-radius:6px;
  }
  .theme-toggle:hover { background:var(--surface); }
  .theme-toggle:disabled { cursor:default; opacity:.6; }
  .theme-toggle.labeled {
    font-size:.75rem; border:1px solid var(--border); padding:.35rem .6rem;
  }
</style>
