<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon, { type IconName } from "./Icon.svelte";
  import { tipAttrs } from "./tip";

  interface Props {
    /** How far in the row sits: 0 for the top level. */
    depth: number;
    /** Open or shut for a row with children; leave it out for a leaf. */
    open?: boolean;
    icon?: IconName;
    selected?: boolean;
    /** Bold: the database the editor runs in. */
    current?: boolean;
    /** Muted words at the end, such as a count. */
    meta?: string | number;
    tip?: string;
    onclick?: () => void;
    ondblclick?: () => void;
    onkeydown?: (event: KeyboardEvent) => void;
    oncontextmenu?: (event: MouseEvent) => void;
    children: Snippet;
  }

  let { depth, open, icon, selected = false, current = false, meta, tip, onclick, ondblclick, onkeydown, oncontextmenu, children }: Props = $props();
</script>

<button
  type="button"
  class="pb-tree-row"
  class:is-selected={selected}
  class:is-current={current}
  style:--depth={depth}
  role="treeitem"
  aria-expanded={open}
  aria-selected={selected || current}
  {...tipAttrs(tip, "start")}
  {onclick}
  {ondblclick}
  {onkeydown}
  {oncontextmenu}
>
  {#if open === undefined}<span class="pb-twist"></span>{:else}<Icon name={open ? "chev-down" : "chev-right"} class="pb-twist" />{/if}
  {#if icon}<Icon name={icon} class="pb-kind" />{/if}{@render children()}
  {#if meta !== undefined}<span class="pb-meta">{meta}</span>{/if}
</button>
