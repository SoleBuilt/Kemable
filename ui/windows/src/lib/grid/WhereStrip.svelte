<script lang="ts">
  import type { ResultWhere } from "../types";
  import Badge from "../ui/Badge.svelte";
  import Button from "../ui/Button.svelte";

  /** A query result's filter as a WHERE to copy, never run (docs/decisions/0011-filtering-a-result.md). */
  let { where, onCopy }: { where: ResultWhere; onCopy: () => void } = $props();
</script>

<div class="pb-where">
  <span class="pb-where-label" data-tip-start="" data-tip="For your own SQL; Paboable filters the rows held and never runs it">To filter on the server</span>
  <span class="pb-where-sql">{where.sql}</span>
  {#if where.leftOut.length > 0}
    <Badge size="sm" tone="warning" tip="A column whose name is empty or shared cannot be named in SQL" tipAt="end">{where.leftOut.length} left out</Badge>
  {/if}
  <Button variant="ghost" size="sm" icon="copy" tip="Copy the WHERE" tipAt="end" onclick={onCopy}>Copy WHERE</Button>
</div>
