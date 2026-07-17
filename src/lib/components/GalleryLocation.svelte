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

<div class="gallery">
  <div class="hdr">
    <span class="lbl">Gallery folder</span>
    <button onclick={change} disabled={!$settings || busy}>Change…</button>
  </div>
  <div class="row" title={$settings?.gallery_dir ?? ""}>
    <span class="path">{$settings?.gallery_dir ?? "…"}</span>
    <button class="open" onclick={openCurrent} disabled={!$settings} aria-label="Open folder" title="Open folder">📂</button>
  </div>
  {#if error}<span class="err" title={error}>Couldn't open folder: {error}</span>{/if}
</div>

<style>
  .gallery { font-size:.75rem; border-top:1px solid var(--border); padding:.45rem .2rem 0; display:flex; flex-direction:column; gap:.25rem; }
  .hdr { display:flex; align-items:center; justify-content:space-between; }
  .lbl { opacity:.6; }
  .row { display:flex; align-items:center; gap:.4rem; font-family:monospace; opacity:.9; }
  .path { flex:1; min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  button { font:inherit; font-size:.72rem; padding:.2rem .5rem; cursor:pointer; }
  button:disabled { opacity:.5; cursor:default; }
  .open { flex:0 0 auto; padding:.1rem .4rem; background:none; border:none; cursor:pointer; line-height:1; }
  .err { color:var(--danger); overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
</style>
