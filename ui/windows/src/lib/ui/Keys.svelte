<script lang="ts">
  /** A shortcut, one key cap per key: ["Ctrl", "K"]. */
  let { keys, class: extra = "" }: { keys: string[]; class?: string } = $props();
</script>

<span class={["pb-keys", extra]}>{#each keys as key, index (index)}<kbd class="pb-kbd">{key}</kbd>{/each}</span>
