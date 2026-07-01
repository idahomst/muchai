<script module lang="ts">
  let counter = 0;
</script>

<script lang="ts">
  let { text, label = "More info" }: { text: string; label?: string } = $props();
  let open = $state(false);
  const tipId = `hint-${counter++}`;

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") open = false;
  }
</script>

<span class="wrap" onmouseenter={() => (open = true)} onmouseleave={(e) => { if (!(e.currentTarget as HTMLElement).contains(document.activeElement)) open = false; }} role="presentation">
  <button
    type="button"
    class="info"
    aria-label={label}
    aria-describedby={open ? tipId : undefined}
    onclick={() => (open = !open)}
    onfocus={() => (open = true)}
    onblur={() => (open = false)}
    {onkeydown}
  >ⓘ</button>
  {#if open}
    <span class="tip" role="tooltip" id={tipId}>{text}</span>
  {/if}
</span>

<style>
  .wrap { position:relative; display:inline-flex; }
  .info { font:inherit; font-size:.7rem; line-height:1; padding:0 .15rem; margin:0;
    background:none; border:none; color:var(--text-muted); cursor:help; opacity:.75; }
  .info:hover, .info:focus-visible { color:var(--accent-bright); opacity:1; }
  .tip { position:absolute; z-index:20; top:calc(100% + 4px); left:0;
    width:max-content; max-width:220px; padding:.4rem .55rem;
    background:var(--surface); color:var(--text);
    border:1px solid var(--border); border-radius:6px;
    box-shadow:0 4px 14px var(--overlay);
    font-size:.72rem; line-height:1.35; font-weight:normal; text-align:left;
    white-space:normal; pointer-events:none; }
</style>
