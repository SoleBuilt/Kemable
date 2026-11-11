<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon, { type IconName } from "./Icon.svelte";
  import Keys from "./Keys.svelte";

  interface Props {
    icon?: IconName;
    /** The shortcut, drawn at the end. */
    keys?: string[];
    /** Muted words at the end, in place of the shortcut ("Later"). */
    end?: string;
    danger?: boolean;
    disabled?: boolean;
    onclick?: () => void;
    children: Snippet;
  }

  let { icon, keys, end, danger = false, disabled = false, onclick, children }: Props = $props();
</script>

<button type="button" class={["pb-menu-item", danger && "is-danger"]} role="menuitem" {disabled} {onclick}>
  {#if icon}<Icon name={icon} />{/if}{@render children()}
  {#if end}<span class="pb-end">{end}</span>{:else if keys}<Keys {keys} />{/if}
</button>
