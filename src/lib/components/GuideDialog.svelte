<script lang="ts">
  import { GUIDES, render, enhance, type GuideId } from "../guides";

  // A link to the sibling guide reports upwards rather than swapping a local
  // copy of `id`: the page owns which guide is open, and two owners of that
  // fact would drift the moment anything else opened one.
  let { id, onclose, onnavigate }: {
    id: GuideId;
    onclose: () => void;
    onnavigate: (id: GuideId) => void;
  } = $props();

  const guide = $derived(GUIDES.find((g) => g.id === id));
  const html = $derived(render(id));

  let closeBtn = $state<HTMLButtonElement>();
  let body = $state<HTMLElement>();

  $effect(() => { closeBtn?.focus(); });

  // Runs after {@html} has written `html` into the container. Reading `html`
  // here is what re-runs it when the guide changes, not only on mount.
  $effect(() => {
    html;
    if (!body) return;
    // A guide is read top-down; arriving at the other one already scrolled to
    // where the previous one was left would look like a rendering fault.
    body.scrollTo({ top: 0 });
    enhance(body, onnavigate);
  });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }
</script>

<svelte:window {onkeydown} />

<div class="modal-backdrop" onclick={(e) => { if (e.target === e.currentTarget) onclose(); }} role="presentation">
  <div class="modal guide" role="dialog" aria-modal="true" aria-labelledby="guide-title">
    <div class="modal-head">
      <span class="modal-title" id="guide-title">{guide?.title ?? "Guide"}</span>
      <button class="modal-x" onclick={onclose} aria-label="Close">✕</button>
    </div>

    <div class="modal-body md" bind:this={body}>
      {@html html}
    </div>

    <div class="modal-foot">
      <button class="btn btn-primary spacer" bind:this={closeBtn} onclick={onclose}>Close</button>
    </div>
  </div>
</div>

<style>
  /* Wider than the other dialogs and capped by the viewport rather than by the
     content: a guide is long-form and read by scrolling. */
  .modal.guide { width: min(760px, 94vw); }
  .modal-body { max-height: min(70vh, 640px); overflow-y: auto; }

  /* Everything below styles markdown that Svelte never compiles, so each
     selector has to be :global — scoped CSS cannot reach {@html} output. */

  /* Each guide opens with its own `# Title`, which GitHub needs and the dialog
     header already shows; rendering both would print the title twice. */
  .md :global(h1) { display:none; }
  .md :global(h2) { font-size:14px; font-weight:650; margin:26px 0 8px;
    padding-top:14px; border-top:1px solid var(--border); }
  .md :global(h2:first-of-type) { border-top:none; padding-top:0; margin-top:0; }
  .md :global(h3) { font-size:13px; font-weight:600; margin:16px 0 6px; }
  .md :global(p), .md :global(li) { font-size:13px; line-height:1.55; }
  .md :global(p) { margin:0 0 10px; }
  .md :global(ul), .md :global(ol) { margin:0 0 12px; padding-left:1.3rem;
    display:flex; flex-direction:column; gap:.35rem; }
  .md :global(strong) { font-weight:600; color:var(--text); }
  .md :global(code) { font-family:var(--mono); font-size:11.5px; padding:1px 5px;
    border-radius:4px; background:var(--card); border:1px solid var(--border); }
  /* The ASCII sketch lives in a fenced block and must not wrap — a wrapped
     window diagram is unreadable, so it scrolls sideways instead. */
  .md :global(pre) { margin:0 0 12px; padding:10px 12px; overflow-x:auto;
    background:var(--card); border:1px solid var(--border); border-radius:var(--radius-sm); }
  .md :global(pre code) { padding:0; border:none; background:none;
    font-size:11px; line-height:1.35; white-space:pre; }
  .md :global(blockquote) { margin:0 0 12px; padding:9px 12px;
    background:var(--accent-soft); border:1px solid var(--border);
    border-radius:var(--radius-sm); }
  .md :global(blockquote p:last-child) { margin:0; }
  .md :global(img) { display:block; max-width:100%; height:auto; margin:4px 0 12px;
    border:1px solid var(--border); border-radius:var(--radius-sm); }
  .md :global(a) { color:var(--accent); cursor:pointer; }
  .md :global(table) { border-collapse:collapse; margin:0 0 12px; font-size:12px; }
  .md :global(th), .md :global(td) { text-align:left; padding:4px 12px 4px 0;
    border-bottom:1px solid var(--border); }
  .md :global(th) { font-weight:600; color:var(--text-muted); font-size:11px;
    letter-spacing:.03em; text-transform:uppercase; }
</style>
