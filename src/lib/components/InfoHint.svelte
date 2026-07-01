<script module lang="ts">
  let counter = 0;
</script>

<script lang="ts">
  let { text, label = "More info" }: { text: string; label?: string } = $props();
  let open = $state(false);
  let btn = $state<HTMLButtonElement>();
  let tipX = $state(0);
  let tipY = $state(0);
  const tipId = `hint-${counter++}`;

  // The tooltip is position:fixed so it escapes the settings sidebar's
  // `overflow:hidden auto` clip — otherwise it gets cut off at the panel edge
  // and neighbouring inputs paint over it. Placed just under the trigger and
  // clamped to the viewport so it never runs off the right edge.
  const MAX_W = 220;
  function place() {
    if (!btn) return;
    const r = btn.getBoundingClientRect();
    const margin = 8;
    let x = r.left;
    if (x + MAX_W + margin > window.innerWidth) x = window.innerWidth - MAX_W - margin;
    if (x < margin) x = margin;
    tipX = x;
    tipY = r.bottom + 4;
  }

  function show() {
    place();
    open = true;
  }

  function toggle() {
    if (open) open = false;
    else show();
  }

  // Keep the tooltip glued to the trigger if the panel scrolls or the window
  // resizes while it's open.
  $effect(() => {
    if (!open) return;
    place();
    window.addEventListener("scroll", place, true);
    window.addEventListener("resize", place);
    return () => {
      window.removeEventListener("scroll", place, true);
      window.removeEventListener("resize", place);
    };
  });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") open = false;
  }
</script>

<span class="wrap" onmouseenter={show} onmouseleave={(e) => { if (!(e.currentTarget as HTMLElement).contains(document.activeElement)) open = false; }} role="presentation">
  <button
    type="button"
    class="info"
    bind:this={btn}
    aria-label={label}
    aria-describedby={open ? tipId : undefined}
    onclick={toggle}
    onfocus={show}
    onblur={() => (open = false)}
    {onkeydown}
  >ⓘ</button>
  {#if open}
    <span class="tip" role="tooltip" id={tipId} style="left:{tipX}px; top:{tipY}px;">{text}</span>
  {/if}
</span>

<style>
  .wrap { position:relative; display:inline-flex; }
  .info { font:inherit; font-size:.7rem; line-height:1; padding:0 .15rem; margin:0;
    background:none; border:none; color:var(--text-muted); cursor:help; opacity:.75;
    transform:translateY(-2px); }
  .info:hover, .info:focus-visible { color:var(--accent-bright); opacity:1; }
  .tip { position:fixed; z-index:100; width:max-content; max-width:220px;
    padding:.4rem .55rem;
    background:var(--surface); color:var(--text);
    border:1px solid var(--border); border-radius:6px;
    box-shadow:0 4px 14px var(--overlay);
    font-size:.72rem; line-height:1.35; font-weight:normal; text-align:left;
    white-space:normal; pointer-events:none; }
</style>
