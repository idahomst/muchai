<script lang="ts">
  import { settings, history } from "$lib/stores";
  import { setSettings, pickGalleryDir, openFolder, listHistory } from "$lib/api";

  let busy = $state(false);
  let error = $state<string | null>(null);

  async function openCurrent() {
    const dir = $settings?.gallery_dir;
    if (!dir) return;
    error = null;
    try {
      await openFolder(dir);
    } catch (e) {
      error = String(e);
      console.error("openFolder failed:", e);
    }
  }

  async function change() {
    if (!$settings || busy) return;
    busy = true;
    try {
      const dir = await pickGalleryDir();
      if (!dir) return;
      const next = { ...$settings, gallery_dir: dir };
      await setSettings(next);
      settings.set(next);
      history.set(await listHistory());
    } finally {
      busy = false;
    }
  }
</script>

<div class="loc">
  <span class="lbl">Saved to</span>
  <span class="path" title={$settings?.gallery_dir ?? ""}>{$settings?.gallery_dir ?? "…"}</span>
  <button onclick={openCurrent} disabled={!$settings}>Open folder</button>
  <button onclick={change} disabled={!$settings || busy}>Change…</button>
</div>
{#if error}
  <div class="err" title={error}>Couldn't open folder: {error}</div>
{/if}

<style>
  .loc { display:flex; align-items:center; gap:.5rem; font-size:.75rem;
    padding:.45rem .2rem 0; border-top:1px solid var(--border); }
  .lbl { opacity:.6; flex:0 0 auto; }
  .path { flex:1; min-width:0; overflow:hidden; text-overflow:ellipsis;
    white-space:nowrap; opacity:.9; font-family:monospace; }
  button { flex:0 0 auto; font:inherit; font-size:.72rem; padding:.25rem .55rem; cursor:pointer; }
  button:disabled { opacity:.5; cursor:default; }
  .err { font-size:.72rem; color:#ff6b6b; padding:.25rem .2rem 0;
    overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
</style>
