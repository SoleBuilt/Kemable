<script lang="ts">
  import type { Snippet } from "svelte";
  import { tipAttrs } from "./tip";

  interface Props {
    selected?: boolean;
    tip?: string;
    onclick?: () => void;
    ondblclick?: () => void;
    /** The lines of the entry. */
    children: Snippet;
    /** Buttons under it: Insert, Run, Delete. */
    actions?: Snippet;
  }

  let { selected = false, tip, onclick, ondblclick, children, actions }: Props = $props();
</script>

<div class="pb-list-entry">
  <button type="button" class="pb-list-item" class:is-selected={selected} {...tipAttrs(tip, "start")} {onclick} {ondblclick}>{@render children()}</button>
  {#if actions}<span class="pb-list-actions">{@render actions()}</span>{/if}
</div>
