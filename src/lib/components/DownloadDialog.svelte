<script lang="ts">
  // Placeholder — full implementation lands in Task 9 (DownloadDialog).
  let {
    onclose,
    ondownloaded,
    refresh,
  }: {
    onclose: () => void;
    ondownloaded: (path: string) => void | Promise<void>;
    refresh: () => Promise<void>;
  } = $props();
  // Reference props via typeof so they are not flagged unused; real wiring comes in Task 9.
  type _Unused = typeof ondownloaded | typeof refresh;
</script>

<div class="backdrop">
  <div class="dialog">
    <p>Download dialog coming soon.</p>
    <button onclick={onclose}>Close</button>
  </div>
</div>

<style>
  .backdrop { position:fixed; inset:0; background:rgba(0,0,0,.5); display:flex;
    align-items:center; justify-content:center; }
  .dialog { background:var(--bg, #1e1e1e); padding:1rem; border:1px solid var(--border); border-radius:6px; }
  button { font:inherit; padding:.3rem .6rem; cursor:pointer; margin-top:.5rem; }
</style>
