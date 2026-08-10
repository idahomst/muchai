<script lang="ts">
  import { settings } from "$lib/stores";
  import { setSettings, pickFolder, openFolder } from "$lib/api";

  let busy = $state(false);
  let error = $state<string | null>(null);

  async function persist(next: typeof $settings) {
    if (!next) return;
    await setSettings(next);
    settings.set(next);
  }

  async function addFolder() {
    if (!$settings || busy) return;
    busy = true;
    error = null;
    try {
      const dir = await pickFolder();
      if (!dir) return;
      if ($settings.extra_model_dirs.includes(dir) || dir === $settings.models_dir) return;
      await persist({ ...$settings, extra_model_dirs: [...$settings.extra_model_dirs, dir] });
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function removeFolder(dir: string) {
    if (!$settings || busy) return;
    busy = true;
    error = null;
    try {
      await persist({ ...$settings, extra_model_dirs: $settings.extra_model_dirs.filter((d) => d !== dir) });
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function openDir(dir: string | undefined) {
    if (!dir) return;
    error = null;
    try {
      await openFolder(dir);
    } catch (e) {
      error = String(e);
    }
  }
</script>

<div class="folders">
  <div class="hdr">
    <span class="lbl">Model folders</span>
    <button onclick={addFolder} disabled={!$settings || busy}>Add folder…</button>
  </div>
  <div class="row" title={$settings?.models_dir ?? ""}>
    <span class="path">{$settings?.models_dir ?? "…"}</span>
    <button class="open" onclick={() => openDir($settings?.models_dir)} disabled={!$settings} aria-label="Open folder" title="Open folder">📂</button>
  </div>
  {#each $settings?.extra_model_dirs ?? [] as dir (dir)}
    <div class="row">
      <span class="path" title={dir}>{dir}</span>
      <button class="open" onclick={() => openDir(dir)} disabled={busy} aria-label="Open folder" title="Open folder">📂</button>
      <button class="x" onclick={() => removeFolder(dir)} disabled={busy} aria-label="Remove folder">×</button>
    </div>
  {/each}
  {#if error}<span class="err">{error}</span>{/if}
</div>

<style>
  /* No border-top: the section header above draws the divider now. */
  .folders { font-size:.75rem; padding:.45rem .2rem 0; display:flex; flex-direction:column; gap:.25rem; }
  .hdr { display:flex; align-items:center; justify-content:space-between; }
  .lbl { opacity:.6; }
  .row { display:flex; align-items:center; gap:.4rem; font-family:monospace; opacity:.9; }
  .path { flex:1; min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  button { font:inherit; font-size:.72rem; padding:.2rem .5rem; cursor:pointer; }
  button:disabled { opacity:.5; cursor:default; }
  .open, .x { flex:0 0 auto; padding:.1rem .4rem; background:none; border:none; cursor:pointer; line-height:1; }
  .err { color:var(--danger); opacity:1; }
</style>
