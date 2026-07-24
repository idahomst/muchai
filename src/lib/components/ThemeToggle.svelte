<script lang="ts">
  import { settings } from "$lib/stores";
  import { setSettings } from "$lib/api";
  import { applyTheme } from "$lib/theme";

  // Three presentations of the same control:
  //  • default   — compact ☀/☽ icon button (app header)
  //  • labeled    — self-explanatory verb button ("☀ Switch to light theme")
  //  • segmented  — a Light | Dark segmented control (Preferences)
  let { labeled = false, segmented = false }: { labeled?: boolean; segmented?: boolean } = $props();

  let busy = $state(false);

  // A not-yet-loaded config is treated as dark.
  const theme = $derived($settings?.theme ?? "dark");

  // Set an explicit theme (optimistic, with rollback). No-op if already active.
  async function setTheme(next: "dark" | "light") {
    if (!$settings || busy || $settings.theme === next) return;
    const prev = $settings;
    const updated = { ...prev, theme: next };
    applyTheme(next);      // instant visual swap + cache
    settings.set(updated); // optimistic
    busy = true;
    try {
      await setSettings(updated);
    } catch {
      settings.set(prev);     // revert the store…
      applyTheme(prev.theme); // …and the visual theme
    } finally {
      busy = false;
    }
  }

  function toggle() {
    setTheme(theme === "light" ? "dark" : "light");
  }
</script>

{#if segmented}
  <div class="seg theme" role="group" aria-label="Theme">
    <button class="seg-item" class:on={theme === "light"} disabled={busy}
      aria-pressed={theme === "light"} onclick={() => setTheme("light")}>Light</button>
    <button class="seg-item" class:on={theme === "dark"} disabled={busy}
      aria-pressed={theme === "dark"} onclick={() => setTheme("dark")}>Dark</button>
  </div>
{:else}
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
{/if}

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
  .seg.theme { max-width:230px; }
  .seg.theme .seg-item:disabled { cursor:default; }
</style>
