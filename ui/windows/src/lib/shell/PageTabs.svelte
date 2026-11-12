<script lang="ts" module>
  import type { IconName } from "../ui/Icon.svelte";

  export interface PageTabItem {
    id: number;
    title: string;
    icon: IconName;
    /** A table page's row count, as the tab shows it. */
    count?: string;
    /** A `.sql` file's page whose text is not saved: a dot where the × is, until hovered. */
    dirty?: boolean;
    /** The tab's tip: a file page's full path. */
    tip?: string;
  }
</script>

<script lang="ts">
  import type { Snippet } from "svelte";
  import Count from "../ui/Count.svelte";
  import Icon from "../ui/Icon.svelte";
  import { showSelected, wheelSideways } from "../ui/strip";
  import { tipAttrs } from "../ui/tip";

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

  let strip: HTMLDivElement | undefined = $state();

  // A new page, or one picked from the palette or by a key, can be past the strip's edge; bring it into view.
  $effect(() => {
    void active;
    void tabs.length;
    showSelected(strip);
  });
</script>

<div class="pb-doctabs">
  <div class="pb-doctabs-left">
    <div class="pb-doctabs-tabs" role="tablist" aria-label="Open pages" tabindex="-1" bind:this={strip} onwheel={wheelSideways}>
      {#each tabs as tab (tab.id)}
        <span
          class="pb-doctab is-link"
          class:is-active={tab.id === active}
          role="tab"
          tabindex="0"
          aria-selected={tab.id === active}
          {...tipAttrs(tab.tip, "start")}
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
            class:is-dirty={tab.dirty}
            aria-label={tab.dirty ? `Close ${tab.title}, not saved` : `Close ${tab.title}`}
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
