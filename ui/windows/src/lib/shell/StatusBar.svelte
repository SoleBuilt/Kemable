<script lang="ts">
  import type { Snippet } from "svelte";

  /** The bar at the window's foot: what the result is on the left, the pager or the app's name on the right. */
  let { children, end }: { children: Snippet; end?: Snippet } = $props();
</script>

<footer class="pb-status">
  {@render children()}
  <span class="pb-spacer"></span>
  {@render end?.()}
</footer>
