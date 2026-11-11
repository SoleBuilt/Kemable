<script lang="ts">
  import type { Snippet } from "svelte";

  /** Muted words in a pane: what is empty and how to fill it. */
  let { children }: { children: Snippet } = $props();
</script>

<p class="pb-side-hint">{@render children()}</p>
