<script lang="ts">
  import Icon from "../ui/Icon.svelte";
  import Keys from "../ui/Keys.svelte";

  /** The search box in the middle of the title bar: it opens the command palette. */
  let { width = 340, open, onclick }: { width?: number; open: boolean; onclick: () => void } = $props();
</script>

<button type="button" class="pb-cmdk-trigger" style:width="{width}px" aria-haspopup="dialog" aria-expanded={open} {onclick}>
  <Icon name="search" /><span class="pb-cmdk-trigger-text">Search connections and commands…</span><Keys keys={["Ctrl", "K"]} />
</button>
