<script module lang="ts">
  let counter = 0;
</script>

<script lang="ts" generics="T">
  import { untrack } from "svelte";
  import type { Snippet } from "svelte";

  // A filterable popover list, and nothing else. It never imports a domain
  // type: items, identity, matching and row markup all come from the caller.
  // That is what makes a third caller — a ControlNet picker — cost nothing
  // structural. Documentation of a design property, not a stub for one.
  let {
    anchor,
    placeholder,
    items,
    key,
    match,
    selected,
    onclose,
    onchoose,
    row,
    action,
    empty,
    footer,
  }: {
    /** The element the popover hangs off. Clicks on it never close the popover —
     *  the trigger owns its own toggle. */
    anchor: HTMLElement | undefined;
    placeholder: string;
    items: T[];
    /** Stable identity, for the keyed each and for row element ids. */
    key: (item: T) => string;
    /** `query` arrives trimmed and lower-cased. */
    match: (item: T, query: string) => boolean;
    selected?: (item: T) => boolean;
    onclose: () => void;
    /** Fired on Enter or a row-body click. **The popover does not close itself
     *  here** — the caller decides. ModelSelector closes; LoraSelector stays
     *  open so several LoRAs can be ticked in one visit. */
    onchoose: (item: T) => void;
    row: Snippet<[T]>;
    action?: Snippet<[T]>;
    empty?: Snippet;
    footer?: Snippet;
  } = $props();

  const uid = `pp${counter++}`;

  let query = $state("");
  let highlight = $state(0);
  let boxEl = $state<HTMLElement>();
  let inputEl = $state<HTMLInputElement>();
  let x = $state(0);
  let y = $state(0);
  let w = $state(0);

  const filtered = $derived(items.filter((i) => match(i, query.trim().toLowerCase())));

  // Move the popover to the end of <body> so it is a top-level layer, then drop
  // it on teardown. Scoped styles still apply — the node keeps its Svelte
  // scoping attribute and the stylesheet lives in <head>. Same approach as
  // InfoHint, and mandatory for the same two reasons: `.controls` is
  // `overflow:hidden` so an absolutely positioned 560px popover would be
  // clipped to the 340px column, and any ancestor with `opacity < 1` creates a
  // stacking context that would capture a `position:fixed` descendant.
  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return { destroy() { node.remove(); } };
  }

  const MAX_W = 560;
  const MARGIN = 16;
  const GAP = 6;

  function place() {
    if (!anchor) return;
    const r = anchor.getBoundingClientRect();
    w = Math.min(MAX_W, window.innerWidth - MARGIN * 2);
    let left = r.left;
    if (left + w + MARGIN > window.innerWidth) left = window.innerWidth - w - MARGIN;
    x = Math.max(MARGIN, left);
    // Open below the anchor; flip above only if there is no room below and
    // there is room above — otherwise a short viewport would push it off the
    // top instead of the bottom.
    const h = boxEl?.offsetHeight ?? 0;
    const below = r.bottom + GAP;
    const above = r.top - h - GAP;
    y = h > 0 && below + h + MARGIN > window.innerHeight && above > MARGIN ? above : below;
  }

  // Re-measure whenever the height can have changed: at mount (boxEl lands
  // after the first paint, so the first pass has no height to work with) and
  // every time filtering shortens the list.
  $effect(() => {
    void filtered.length;
    void boxEl;
    place();
  });

  // Stay glued to the anchor while the sidebar scrolls under it. Capture phase:
  // `.panel-body` scrolls, not the window, and scroll does not bubble.
  $effect(() => {
    queueMicrotask(() => inputEl?.focus());
    window.addEventListener("scroll", place, true);
    window.addEventListener("resize", place);
    return () => {
      window.removeEventListener("scroll", place, true);
      window.removeEventListener("resize", place);
    };
  });

  // Start on the chosen row, once. Untracked so typing in the filter doesn't
  // yank the highlight back.
  $effect(() => {
    const i = untrack(() => filtered).findIndex((it) => selected?.(it) ?? false);
    highlight = i >= 0 ? i : 0;
  });

  // Keep the highlight in range as filtering shrinks the list.
  $effect(() => {
    if (highlight > filtered.length - 1) highlight = Math.max(0, filtered.length - 1);
  });

  $effect(() => {
    void highlight;
    (boxEl?.querySelector(".rowwrap.active") as HTMLElement | null)
      ?.scrollIntoView({ block: "nearest" });
  });

  function onFilterKey(e: KeyboardEvent) {
    if (e.key === "ArrowDown") { e.preventDefault(); highlight = Math.min(highlight + 1, filtered.length - 1); }
    else if (e.key === "ArrowUp") { e.preventDefault(); highlight = Math.max(highlight - 1, 0); }
    else if (e.key === "Enter") { e.preventDefault(); const it = filtered[highlight]; if (it) onchoose(it); }
  }

  // Non-modal: outside pointerdown and Escape close it. Escape stops
  // propagating so it closes the popover only — nothing behind it also reacts.
  $effect(() => {
    const onDown = (e: PointerEvent) => {
      const t = e.target as Node;
      if (boxEl?.contains(t) || anchor?.contains(t)) return;
      onclose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") { e.stopPropagation(); onclose(); }
    };
    document.addEventListener("pointerdown", onDown, true);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onDown, true);
      document.removeEventListener("keydown", onKey);
    };
  });
</script>

<div class="pop" bind:this={boxEl} use:portal style="left:{x}px; top:{y}px; width:{w}px;">
  <div class="search">
    <span class="mag" aria-hidden="true">⌕</span>
    <input
      class="filter"
      aria-label={placeholder}
      {placeholder}
      bind:value={query}
      bind:this={inputEl}
      onkeydown={onFilterKey} />
  </div>

  <div
    class="list"
    role="listbox"
    tabindex="-1"
    aria-activedescendant={filtered[highlight] ? `${uid}-${key(filtered[highlight])}` : undefined}
  >
    {#if filtered.length === 0}
      <div class="empty">{#if empty}{@render empty()}{/if}</div>
    {:else}
      {#each filtered as item, idx (key(item))}
        <div class="rowwrap" class:active={idx === highlight} class:on={selected?.(item) ?? false}>
          <button
            class="rowbody"
            id={`${uid}-${key(item)}`}
            role="option"
            aria-selected={selected?.(item) ?? false}
            onclick={() => onchoose(item)}
            onmouseenter={() => (highlight = idx)}
          >{@render row(item)}</button>
          {#if action}{@render action(item)}{/if}
        </div>
      {/each}
    {/if}
  </div>

  {#if footer}<div class="foot">{@render footer()}</div>{/if}
</div>

<style>
  /* Above the download and engine toasts (40/41), below the modal backdrop
     (50). A picker is always closed before a modal opens, so they never
     coexist — but if that ordering is ever broken, the modal still wins. */
  .pop { position:fixed; z-index:45; box-sizing:border-box;
    border-radius:var(--radius); overflow:hidden; background:var(--surface);
    border:1px solid var(--border-strong);
    box-shadow:0 16px 40px var(--overlay), 0 2px 8px var(--overlay-soft); }

  .search { display:flex; align-items:center; gap:8px; padding:9px 11px;
    border-bottom:1px solid var(--border); }
  .mag { font-size:13px; color:var(--text-muted); }
  .filter { flex:1; min-width:0; background:none; border:none; color:var(--text);
    font:inherit; font-size:13px; padding:0; outline:none; }
  .filter::placeholder { color:var(--text-muted); }

  .list { padding:6px; max-height:min(380px, 60vh); overflow-y:auto;
    scrollbar-width:thin; scrollbar-color:var(--border-strong) transparent; }

  /* The row is a wrapper, not a button: the trailing action is a real button
     and cannot be nested inside one. */
  .rowwrap { display:flex; align-items:center; gap:2px; position:relative;
    border-radius:var(--radius-sm); }
  .rowwrap + .rowwrap { margin-top:2px; }
  .rowwrap:hover { background:var(--card-hover); }
  .rowwrap.active { background:var(--card-hover); box-shadow:inset 0 0 0 1px var(--border-strong); }
  .rowwrap.on { background:var(--accent-soft); }
  .rowwrap.on::before { content:""; position:absolute; left:0; top:8px; bottom:8px;
    width:3px; border-radius:0 3px 3px 0; background:var(--accent); }

  /* min-width:0 is the whole point of the rework: without it a nowrap sibling
     group inside the caller's row markup cannot shrink, so the name absorbs
     the entire deficit and truncates to one character. */
  .rowbody { flex:1 1 auto; min-width:0; display:flex; align-items:center; gap:10px;
    padding:8px 10px; border-radius:var(--radius-sm); cursor:pointer;
    background:transparent; border:none; color:var(--text); text-align:left; font:inherit; }

  .empty { font-size:13px; color:var(--text-muted); margin:6px 8px; }

  .foot { display:flex; align-items:center; gap:4px; padding:7px;
    border-top:1px solid var(--border); }
</style>
