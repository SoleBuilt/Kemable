<script lang="ts">
  /** A connection's colour as a dot. `color` names a token (`red` reads `--conn-red`); none is grey. */
  let { color }: { color?: string | null } = $props();
</script>

<span class="pb-dot" style:--conn={color ? `var(--conn-${color})` : undefined}></span>
