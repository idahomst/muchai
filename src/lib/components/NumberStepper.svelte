<script lang="ts">
  let {
    value = $bindable(),
    min = -Infinity,
    max = Infinity,
    step = 1,
    id,
    ariaLabel,
  }: {
    value: number;
    min?: number;
    max?: number;
    step?: number;
    id?: string;
    ariaLabel?: string;
  } = $props();

  const clamp = (n: number) => Math.min(max, Math.max(min, n));
  // Round to the step grid so 0.5-steps don't accumulate float drift.
  const snap = (n: number) => {
    const decimals = (String(step).split(".")[1] ?? "").length;
    return decimals ? Number(n.toFixed(decimals)) : n;
  };

  function bump(dir: 1 | -1) { value = clamp(snap((value ?? 0) + dir * step)); }

  function onInput(e: Event) {
    const raw = (e.currentTarget as HTMLInputElement).value;
    const n = Number(raw);
    if (raw !== "" && !Number.isNaN(n)) value = clamp(n);
  }
</script>

<div class="num">
  <input
    class="val"
    type="number"
    {id}
    {min}
    {max}
    {step}
    aria-label={ariaLabel}
    bind:value
    oninput={onInput}
  />
  <button type="button" class="stp" tabindex="-1" aria-label="Decrease" onclick={() => bump(-1)}>−</button>
  <button type="button" class="stp" tabindex="-1" aria-label="Increase" onclick={() => bump(1)}>+</button>
</div>

<style>
  .num { display: flex; align-items: center; background: var(--card);
    border: 1px solid var(--border); border-radius: var(--radius-sm); overflow: hidden; }
  .num:focus-within { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .val { flex: 1; min-width: 0; background: none; border: none; color: var(--text);
    font: inherit; font-size: 13px; padding: 9px 11px; outline: none;
    font-variant-numeric: tabular-nums; }
  /* Hide the native spinners — the −/+ buttons replace them. */
  .val::-webkit-inner-spin-button, .val::-webkit-outer-spin-button { -webkit-appearance: none; margin: 0; }
  .val { -moz-appearance: textfield; appearance: textfield; }
  .stp { width: 30px; align-self: stretch; display: grid; place-items: center;
    color: var(--text-muted); cursor: pointer; font-size: 14px; background: transparent;
    border: none; border-left: 1px solid var(--border); border-radius: 0; }
  .stp:hover { background: var(--card-hover); color: var(--text); }
</style>
