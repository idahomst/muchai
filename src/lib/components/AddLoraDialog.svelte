<script lang="ts">
  import { addUrlLora, addLocalLora, detectLoraFamily, pickLoraFile, editLora, listFamilies } from "../api";
  import { refreshLoras, downloadBusy, downloadProgress, downloadError, runDownload, engineInstalling } from "../stores";
  import DownloadProgressBar from "./DownloadProgressBar.svelte";
  import type { AddedLora } from "../types";

  let { onClose }: { onClose: () => void } = $props();

  type Tab = "url" | "local";
  let tab = $state<Tab>("url");
  let error = $state<string | null>(null);
  let busy = $state(false);

  let families = $state<string[]>([]);
  $effect(() => {
    listFamilies().then((fs) => (families = fs)).catch(() => (families = []));
  });

  // --- URL tab -------------------------------------------------------------
  let url = $state("");
  let urlName = $state("");
  // Set when a download finished but the detector couldn't settle on one
  // family. The entry already exists; this is the follow-up question.
  let pending = $state<AddedLora | null>(null);
  let pendingFamily = $state("");

  async function runUrl() {
    error = null;
    // runDownload owns busy/progress/error so the bar keeps working even if
    // this dialog is closed mid-download. It also refreshes the model library —
    // wasted work for a LoRA, but a cheap directory scan, and not worth
    // branching the shared helper for.
    const added = await runDownload(() => addUrlLora(url, urlName));
    if (added === null) return;
    await refreshLoras();
    if (added.lora.family === "") {
      // Ambiguous or undetectable — ask, seeding the dropdown from the
      // detector's candidates when it had any.
      pending = added;
      pendingFamily = added.candidates[0] ?? "";
    } else {
      onClose();
    }
  }

  async function confirmPendingFamily() {
    const p = pending;
    if (!p) return;
    busy = true;
    error = null;
    try {
      await editLora(p.lora.id, p.lora.display_name, pendingFamily);
      await refreshLoras();
      onClose();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  // --- Local tab -----------------------------------------------------------
  let localPath = $state("");
  let localName = $state("");
  let localFamily = $state("");
  let localCandidates = $state<string[]>([]);
  let detected = $state(false);

  async function pickLocal() {
    const picked = await pickLoraFile();
    if (!picked) return;
    localPath = picked;
    localName = "";
    detected = false;
    error = null;
    try {
      // The file is already on disk, so the family can be settled before
      // anything is committed — unlike the URL tab.
      localCandidates = await detectLoraFamily(picked);
      localFamily = localCandidates.length === 1 ? localCandidates[0] : "";
      detected = true;
    } catch (e) {
      localCandidates = [];
      localFamily = "";
      detected = true;
      error = String(e);
    }
  }

  async function runLocal() {
    busy = true;
    error = null;
    try {
      await addLocalLora(localPath, localName, localFamily);
      await refreshLoras();
      onClose();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  const familyOptions = $derived(
    localCandidates.length > 0 ? localCandidates : families,
  );
  const pendingOptions = $derived(
    pending && pending.candidates.length > 0 ? pending.candidates : families,
  );
</script>

<div class="modal-backdrop" onclick={(e) => { if (e.target === e.currentTarget) onClose(); }} role="presentation">
  <div class="modal" role="dialog" aria-modal="true" aria-label="Add a LoRA">
    <div class="modal-head">
      <span class="modal-title">Add a LoRA</span>
      <button class="modal-x" onclick={onClose} aria-label="Close">✕</button>
    </div>

    <div class="modal-body">
      {#if pending}
        <p class="ask">
          Downloaded “{pending.lora.display_name}”. Which model family is it for?
        </p>
        <p class="hint">
          {pending.candidates.length > 1
            ? "Its layout matches more than one family, so it can't be told apart automatically."
            : "Its layout didn't match any known family."}
        </p>
        <div class="dlg-field">
          <p class="microlabel">Base family</p>
          <select class="dlg-select" bind:value={pendingFamily}>
            <option value="">Unknown</option>
            {#each pendingOptions as f}<option value={f}>{f}</option>{/each}
          </select>
        </div>
        {#if error}<p class="err">{error}</p>{/if}
        <button class="btn btn-primary" disabled={busy} onclick={confirmPendingFamily}>Save</button>
      {:else}
        <div class="seg" role="group" aria-label="LoRA source">
          <button class="seg-item" class:on={tab === "url"} aria-pressed={tab === "url"} onclick={() => (tab = "url")}>URL</button>
          <button class="seg-item" class:on={tab === "local"} aria-pressed={tab === "local"} onclick={() => (tab = "local")}>Local file</button>
        </div>

        {#if error}<p class="err">{error}</p>{/if}
        {#if $downloadError}<p class="err">{$downloadError}</p>{/if}

        {#if tab === "url"}
          <div class="dlg-field">
            <p class="microlabel">URL (https)</p>
            <input class="dlg-input" bind:value={url} placeholder="https://civitai.com/models/…" />
          </div>
          <p class="hint">
            A Civitai link also brings across the LoRA's name and its trigger words.
          </p>
          <div class="dlg-field">
            <p class="microlabel">Name (optional)</p>
            <input class="dlg-input" bind:value={urlName} placeholder="Leave blank to use the source's name" />
          </div>
          <button class="btn btn-primary" disabled={$downloadBusy || $engineInstalling || !url.startsWith("https://")} onclick={runUrl}>
            Download &amp; add
          </button>
          {#if $downloadBusy}<div class="progress"><DownloadProgressBar progress={$downloadProgress} /></div>{/if}
        {:else}
          <div class="dlg-field">
            <p class="microlabel">File</p>
            <div class="pick">
              <input class="dlg-input" readonly value={localPath} placeholder="Choose a .safetensors…" />
              <button class="btn btn-ghost" onclick={pickLocal}>Browse…</button>
            </div>
          </div>
          <p class="hint">The file stays where it is — MuchAI links to it rather than copying it.</p>
          {#if detected}
            <div class="dlg-field">
              <p class="microlabel">Base family</p>
              <select class="dlg-select" bind:value={localFamily}>
                <option value="">Unknown</option>
                {#each familyOptions as f}<option value={f}>{f}</option>{/each}
              </select>
            </div>
            {#if localCandidates.length === 0}
              <p class="hint">Couldn't tell from the file — pick the family it was trained for.</p>
            {:else if localCandidates.length > 1}
              <p class="hint">Matches more than one family; pick the right one.</p>
            {/if}
          {/if}
          <div class="dlg-field">
            <p class="microlabel">Name (optional)</p>
            <input class="dlg-input" bind:value={localName} placeholder="Leave blank to use the filename" />
          </div>
          <!-- Gated on $downloadBusy too: a URL add reserves its pool name only
               when the download finishes (it streams to a sibling .part file),
               so a local add started mid-download could be handed the same
               stem and the two would collide on one file. -->
          <button class="btn btn-primary" disabled={busy || $downloadBusy || $engineInstalling || localPath === ""} onclick={runLocal}>Add</button>
        {/if}
      {/if}
    </div>
  </div>
</div>

<style>
  .ask { font-size: 13.5px; font-weight: 600; margin: 0 0 6px; }
  .hint { font-size: 11.5px; color: var(--text-muted); margin: 0 0 14px; line-height: 1.45; }
  .pick { display: flex; gap: 8px; }
  .pick .dlg-input { flex: 1; }
  .err { color: var(--danger); font-size: 12px; margin: 0 0 10px; overflow-wrap: anywhere; }
  .progress { margin-top: 12px; }
</style>
