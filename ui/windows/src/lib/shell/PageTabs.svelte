<script lang="ts" module>
  import type { IconName } from "../ui/Icon.svelte";

  export interface PageTabItem {
    id: number;
    title: string;
    icon: IconName;
    /** A table page's row count, as the tab shows it. */
    count?: string;
  }
</script>

<script lang="ts">
  import type { Snippet } from "svelte";
  import Count from "../ui/Count.svelte";
  import Icon from "../ui/Icon.svelte";

  /**
   * The page bar (docs/decisions/0013-pages-and-tabs.md): the open pages on
   * the left, scrolling sideways, then +; the tools of the page shown on the right.
   */
  interface Props {
    tabs: PageTabItem[];
    active: number;
    onSelect: (id: number) => void;
    onClose: (id: number) => void;
    onNew: () => void;
    tools?: Snippet;
  }

  let { tabs, active, onSelect, onClose, onNew, tools }: Props = $props();

  // A mouse wheel scrolls the tabs sideways, as a browser's tab strip does.
  function onWheel(event: WheelEvent) {
    const strip = event.currentTarget as HTMLElement;
    if (event.deltaY !== 0 && strip.scrollWidth > strip.clientWidth) strip.scrollLeft += event.deltaY;
  }
</script>

<div class="pb-doctabs">
  <div class="pb-doctabs-left">
    <div class="pb-doctabs-tabs" role="tablist" aria-label="Open pages" tabindex="-1" onwheel={onWheel}>
      {#each tabs as tab (tab.id)}
        <span
          class="pb-doctab is-link"
          class:is-active={tab.id === active}
          role="tab"
          tabindex="0"
          aria-selected={tab.id === active}
          onclick={() => onSelect(tab.id)}
          onauxclick={(event) => {
            if (event.button === 1) onClose(tab.id);
          }}
          onkeydown={(event) => {
            if (event.key === "Enter" || event.key === " ") {
              event.preventDefault();
              onSelect(tab.id);
            }
          }}
        >
          <Icon name={tab.icon} />{tab.title}
          {#if tab.count}<Count value={tab.count} />{/if}
          <button
            type="button"
            class="pb-doctab-x"
            aria-label="Close {tab.title}"
            onclick={(event) => {
              event.stopPropagation();
              onClose(tab.id);
            }}><Icon name="x" /></button
          >
        </span>
      {/each}
    </div>
    <button type="button" class="pb-doctab-new" aria-label="New query tab" data-tip="New query  Ctrl+T" onclick={onNew}><Icon name="plus" /></button>
  </div>
  <div class="pb-doctabs-tools">{@render tools?.()}</div>
</div>
