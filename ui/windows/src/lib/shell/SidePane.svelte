<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon from "../ui/Icon.svelte";

  /**
   * One pane of the sidebar, as in VS Code (docs/decisions/0013-pages-and-tabs.md):
   * the main one fills the sidebar under a fixed header; the others fold.
   */
  interface Props {
    title: string;
    /** The main pane: always open, its header does not fold. */
    main?: boolean;
    open?: boolean;
    count?: string | number | null;
    /** Icon buttons at the header's end (main pane only). */
    actions?: Snippet;
    children: Snippet;
  }

  let { title, main = false, open = $bindable(false), count = null, actions, children }: Props = $props();
  const shown = $derived(main || open);
</script>

<section class={["pb-pane", shown && "is-open", main && "is-main"]} aria-label={title}>
  {#if main}
    <div class="pb-pane-head is-static">
      {title}
      {#if actions}<span class="pb-pane-actions">{@render actions()}</span>{/if}
    </div>
  {:else}
    <button type="button" class="pb-pane-head" aria-expanded={open} onclick={() => (open = !open)}>
      <Icon name={open ? "chev-down" : "chev-right"} class="pb-twist" />{title}{#if count !== null}<span class="pb-pane-count">{count}</span>{/if}
    </button>
  {/if}
  <!-- Kept mounted while folded, so the header's count stays right. -->
  <div class="pb-pane-body" hidden={!shown}>{@render children()}</div>
</section>
