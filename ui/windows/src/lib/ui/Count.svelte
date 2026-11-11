<script lang="ts">
  /** A count beside a label: muted, or a filled red bubble when it counts errors. */
  let { value, alert = false }: { value: string | number; alert?: boolean } = $props();
</script>

<span class={alert ? "pb-count-badge" : "pb-count"}>{value}</span>
