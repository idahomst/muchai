<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { get } from "svelte/store";
  import {
    settings, engineUpdateTag, engineInstalling, enginePct, downloadBusy, engineError,
  } from "$lib/stores";
  import {
    engineStatus, engineCheckUpdate, engineChangelog, engineApplyUpdate,
    engineSelect, onEngineDownloadProgress, setSettings, cancelDownload, getSettings,
  } from "$lib/api";
  import type { EngineStatus, EngineUpdate, EngineSelection, ChangeEntry } from "$lib/types";
  import { INSUFFICIENT_SPACE_PREFIX } from "$lib/types";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { formatBytes } from "$lib/modelFormat";

  let status = $state<EngineStatus | null>(null);
  let update = $state<EngineUpdate | null>(null);
  let changes = $state<ChangeEntry[] | null>(null);
  let showAllChanges = $state(false);
  let checking = $state(false);
  // Errors live in a store, not here: an install outlives this component, and a
  // failure written into a destroyed panel is a failure the user never sees.
  let done = $state<string | null>(null);
  // Set once a check has completed and found nothing, so "Up to date" appears
  // only after we actually looked — never as an unearned claim on first paint.
  let upToDate = $state(false);
  let unlisten: UnlistenFn | null = null;
  // Set before `unlisten` can be: `onMount` awaits three round trips before it
  // registers, and closing Preferences in the meantime would otherwise leave a
  // listener nobody can ever remove.
  let destroyed = false;

  onMount(async () => {
    // A status the backend couldn't produce leaves the card in its "…" state
    // rather than throwing: nothing here is worth an error dialog.
    status = await engineStatus().catch(() => null);
    // Opening this section is the user noticing the badge: remember the tag so
    // it doesn't come back at the next launch, whether or not they go on to
    // install. `engine_seen_tag` is the one engine field `set_settings` takes
    // from the payload — see merged_settings. The dot itself is cleared in
    // onDestroy, so it survives long enough to point here.
    const seen = $engineUpdateTag;
    if (seen && $settings && $settings.engine_seen_tag !== seen) {
      const next = { ...$settings, engine_seen_tag: seen };
      await setSettings(next).catch(() => {});
      settings.set(next);
    }
    const un = await onEngineDownloadProgress((p) => {
      enginePct.set(p.total ? Math.round((p.downloaded / p.total) * 100) : null);
    });
    if (destroyed) un();
    else unlisten = un;
  });

  onDestroy(() => {
    destroyed = true;
    unlisten?.();
    // Cleared here rather than at mount: the dot on the Preferences header is
    // what points at this section, and clearing it on the way in meant it could
    // never be seen. The tag was already persisted as seen at mount.
    engineUpdateTag.set(null);
    // An install still running owns the error slot from here on — the toast is
    // the only thing that can report it once this panel is gone.
    if (!get(engineInstalling)) engineError.set(null);
  });

  async function check() {
    checking = true;
    engineError.set(null); done = null; changes = null; upToDate = false;
    try {
      update = await engineCheckUpdate();
      // A missing changelog is not a reason to withhold the update itself —
      // the compare endpoint 404s on revisions GitHub has forgotten.
      if (update) changes = await engineChangelog(update.tag).catch(() => null);
      else upToDate = true;
    } catch (e) {
      // A check the user asked for DOES report its failure — unlike the silent
      // background one, they are waiting on an answer.
      engineError.set(String(e));
    } finally {
      checking = false;
    }
  }

  async function install() {
    if (!update) return;
    engineInstalling.set(true);
    engineError.set(null); enginePct.set(0);
    try {
      const commit = await engineApplyUpdate(update.tag);
      done = `Installed ${update.tag} (commit ${commit}).`;
      update = null; changes = null; upToDate = true;
      status = await engineStatus();
      // The install also wrote `engine_seen_tag`. This store is loaded once at
      // mount and posted back whole on every preference change, so leaving it
      // stale means the next theme toggle reverts the mark — and the badge
      // returns at the next launch for the build the user just installed.
      await getSettings().then((s) => settings.set(s)).catch(() => {});
    } catch (e) {
      engineError.set(String(e));
    } finally {
      engineInstalling.set(false);
      enginePct.set(null);
    }
  }

  // One AtomicBool serves every download in the backend, so this button would
  // also stop a model download running behind the dialog. Nothing in the
  // backend enforces that single-flight — the UI does, in both directions: this
  // panel refuses to install while `downloadBusy`, and the model and LoRA
  // dialogs refuse to download while `engineInstalling`. See the note on
  // `engine_apply_update`.
  function cancel() {
    cancelDownload().catch(() => {});
  }

  async function select(sel: EngineSelection) {
    engineError.set(null); done = null;
    // The offer on screen was computed against the engine they are leaving; it
    // is no longer an answer to the question they are now asking.
    update = null; changes = null; upToDate = false;
    try {
      await engineSelect(sel);
      status = await engineStatus();
      if (sel.type === "builtin") done = "Switched back to the built-in engine.";
    } catch (e) {
      engineError.set(String(e));
    }
  }

  // Optimistic, like every other save in this dialog: the checkbox renders from
  // the store, so leaving the store alone on failure would show a ticked box
  // next to an error saying it wasn't saved.
  async function saveAutoCheck(on: boolean) {
    const cur = $settings;
    if (!cur || cur.engine_update_check === on) return;
    settings.set({ ...cur, engine_update_check: on });
    engineError.set(null);
    try {
      await setSettings({ ...cur, engine_update_check: on });
    } catch (e) {
      settings.set(cur);
      engineError.set(String(e));
    }
  }

  // Noise (docs/ci/chore) is real but not why anyone reads a changelog; show
  // the substantive commits and put the rest behind a count.
  const shown = $derived(changes ? changes.filter((c) => c.noteworthy) : []);
  const noiseCount = $derived(changes ? changes.length - shown.length : 0);
</script>

<div class="engine">
  <div class="row">
    <span class="k">In use</span>
    <span class="v" title={status?.path ?? ""}>
      {#if !status}…
      {:else if status.selection.type === "custom"}Your own build{#if status.commit} · commit {status.commit}{/if}
      {:else}{status.tag ?? "unknown"}{#if status.commit} · commit {status.commit}{/if}
      {/if}
    </span>
  </div>

  {#if status?.fell_back}
    <p class="warn" role="status">
      The engine you selected is missing, so MuchAI is using the built-in one.
    </p>
  {/if}

  <!-- Always offered whenever something other than the built-in engine is
       selected, even with nothing downloaded (a custom build counts). This is
       the design's primary safety mechanism and has to be findable by someone
       who is confused and whose images have stopped working — so it is a
       button in plain sight, never behind a menu. -->
  {#if status && (status.selection.type !== "builtin" || status.installed.length > 0)}
    <div class="row">
      <span class="k">Switch to</span>
      <span class="v picks">
        <button class="btn btn-ghost btn-sm" class:on={status.selection.type === "builtin"}
          onclick={() => select({ type: "builtin" })}
          disabled={$engineInstalling || checking}>Built-in</button>
        {#each status.installed as tag (tag)}
          <button class="btn btn-ghost btn-sm"
            class:on={status.selection.type === "downloaded" && status.selection.tag === tag}
            onclick={() => select({ type: "downloaded", tag })}
            disabled={$engineInstalling || checking}>{tag}</button>
        {/each}
      </span>
    </div>
  {/if}

  {#if status && !status.supported}
    <p class="hint">Engine updates are only available on Linux x86_64.</p>
  {:else}
    <div class="actions">
      <button class="btn btn-ghost btn-sm" onclick={check}
        disabled={!status || checking || $engineInstalling}>
        {checking ? "Checking…" : "Check for updates"}
      </button>
      {#if $engineInstalling}
        <span class="prog">{$enginePct === null ? "Installing…" : `Downloading… ${$enginePct}%`}</span>
        <button class="btn btn-ghost btn-sm" onclick={cancel}>Cancel</button>
      {:else if upToDate && !update}
        <span class="prog">Up to date</span>
      {/if}
    </div>
  {/if}

  {#if update}
    <div class="update">
      <p class="uhdr">
        <strong>{update.tag}</strong> is available
        <span class="size">({formatBytes(update.asset_size)})</span>
      </p>
      {#if shown.length > 0}
        <ul class="changes">
          {#each (showAllChanges ? shown : shown.slice(0, 5)) as c}
            <li title={c.subject}>{c.subject}</li>
          {/each}
        </ul>
        {#if shown.length > 5 && !showAllChanges}
          <button class="more" onclick={() => (showAllChanges = true)}>
            Show {shown.length - 5} more
          </button>
        {/if}
        {#if noiseCount > 0}
          <p class="hint">plus {noiseCount} documentation and maintenance commits</p>
        {/if}
      {:else if changes}
        <p class="hint">No user-visible changes listed.</p>
      {/if}
      <button class="btn btn-primary btn-sm install" onclick={install}
        disabled={$engineInstalling || $downloadBusy}>
        Update engine
      </button>
      {#if $downloadBusy}
        <p class="hint">
          A model is downloading. Finish or cancel it first — the two share one
          cancel button, so starting both would leave neither cancellable.
        </p>
      {/if}
    </div>
  {/if}

  <label class="check">
    <input type="checkbox"
      checked={$settings?.engine_update_check ?? true}
      disabled={!$settings}
      onchange={(e) => saveAutoCheck(e.currentTarget.checked)} />
    <span class="check-box"></span>
    <span>Check for engine updates <em>— once a day, in the background</em></span>
  </label>

  {#if done}<p class="ok" role="status">{done}</p>{/if}
  {#if $engineError}
    <p class="err" role="alert">{$engineError}</p>
    <!-- The backend prefixes out-of-space failures so the model downloader can
         raise its reclaim panel. That panel lives in the model dialog and does
         not belong here, so point at it rather than duplicating it. -->
    {#if $engineError.includes(INSUFFICIENT_SPACE_PREFIX)}
      <p class="hint">You can free space from the model list: New model → Reclaim space.</p>
    {/if}
  {/if}
</div>

<style>
  .engine { display:flex; flex-direction:column; gap:8px; }
  .row { display:flex; align-items:baseline; gap:10px; font-size:12.5px; }
  .k { flex:0 0 72px; color:var(--text-muted); }
  .v { flex:1; min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;
    font-family:var(--mono); font-size:11.5px; }
  .picks { display:flex; flex-wrap:wrap; gap:6px; font-family:inherit; }
  /* The selected engine, not a hover state: which binary runs has to be legible
     at a glance in a section the user only opens when something is wrong. */
  .picks .on { border-color:var(--accent); color:var(--accent); }
  .actions { display:flex; align-items:center; gap:10px; flex-wrap:wrap; }
  .update { border:1px solid var(--border); border-radius:var(--radius-sm);
    background:var(--card); padding:10px 12px; display:flex; flex-direction:column; gap:6px; }
  .uhdr { margin:0; font-size:12.5px; }
  .size { color:var(--text-muted); }
  .changes { margin:0; padding-left:16px; display:flex; flex-direction:column; gap:3px;
    font-size:12px; color:var(--text-muted); }
  .changes li { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .more { align-self:flex-start; background:none; border:none; padding:0; font:inherit;
    font-size:11.5px; color:var(--accent); text-decoration:underline; cursor:pointer; }
  .install { align-self:flex-start; margin-top:2px; }
  .hint { margin:0; font-size:11.5px; color:var(--text-muted); }
  .prog { font-size:12px; color:var(--text-muted); }
  .warn { margin:0; padding:8px 10px; border-radius:var(--radius-sm); font-size:12px;
    line-height:1.4; background:var(--warn-tint); color:var(--warn); }
  .ok { margin:0; font-size:12px; color:var(--text-muted); }
  .err { margin:0; font-size:12px; color:var(--danger-soft); overflow-wrap:anywhere; }
  .check em { color:var(--text-muted); font-style:normal; }
</style>
