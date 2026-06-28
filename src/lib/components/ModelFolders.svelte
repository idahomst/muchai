<script lang="ts">
  import { settings, models } from "$lib/stores";
  import { setSettings, pickFolder, listModels } from "$lib/api";

  let busy = $state(false);
  let error = $state<string | null>(null);

  async function persist(next: typeof $settings) {
    if (!next) return;
    await setSettings(next);
    settings.set(next);
    models.set(await listModels());
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
</script>

<div class="folders">
  <div class="hdr">
    <span class="lbl">Model folders</span>
    <button onclick={addFolder} disabled={!$settings || busy}>Add folder…</button>
  </div>
  <div class="primary" title={$settings?.models_dir ?? ""}>
    {$settings?.models_dir ?? "…"} <span class="tag">primary · downloads</span>
  </div>
  {#each $settings?.extra_model_dirs ?? [] as dir (dir)}
    <div class="extra">
      <span class="path" title={dir}>{dir}</span>
      <button class="x" onclick={() => removeFolder(dir)} disabled={busy} aria-label="Remove folder">×</button>
    </div>
  {/each}
  {#if error}<span class="err">{error}</span>{/if}
</div>

<style>
  .folders { font-size:.75rem; border-top:1px solid var(--border); padding:.45rem .2rem 0; display:flex; flex-direction:column; gap:.25rem; }
  .hdr { display:flex; align-items:center; justify-content:space-between; }
  .lbl { opacity:.6; }
  .primary, .extra { display:flex; align-items:center; gap:.4rem; font-family:monospace; opacity:.9; }
  .path, .primary { flex:1; min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .tag { font-family:inherit; opacity:.5; }
  button { font:inherit; font-size:.72rem; padding:.2rem .5rem; cursor:pointer; }
  button:disabled { opacity:.5; cursor:default; }
  .x { padding:.1rem .4rem; }
  .err { color:#ff6b6b; opacity:1; }
</style>
