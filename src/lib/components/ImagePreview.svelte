<script lang="ts">
  import { currentImage } from "../stores";
</script>

<div class="preview">
  {#if $currentImage}
    <img src={$currentImage} alt="generated result" />
  {:else}
    <p class="empty">Your generated image will appear here.</p>
  {/if}
</div>

<style>
  .preview { flex:1; display:flex; align-items:center; justify-content:center;
    background:rgba(0,0,0,.2); border-radius:8px; overflow:hidden; }
  img { max-width:100%; max-height:100%; object-fit:contain; }
  .empty { opacity:.5; }
</style>
