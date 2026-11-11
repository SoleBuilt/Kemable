<script lang="ts">
  import type { Snippet } from "svelte";

  /** One line of a settings page: what it is and a hint on the left, its control on the right. */
  let { title, hint, children }: { title: string; hint?: string; children: Snippet } = $props();
</script>

<div class="pb-setting">
  <div class="pb-setting-text">
    <span>{title}</span>
    {#if hint}<span class="pb-hint">{hint}</span>{/if}
  </div>
  {@render children()}
</div>
