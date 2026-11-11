<script lang="ts">
  import type { Snippet } from "svelte";

  let { label, children }: { label: string; children: Snippet } = $props();
</script>

<div class="pb-tabs" role="tablist" aria-label={label}>{@render children()}</div>
