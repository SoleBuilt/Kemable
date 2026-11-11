<script lang="ts" module>
  export interface ResultTabItem {
    key: number;
    label: string;
    /** The rows shown, formatted. */
    count: string;
    /** It stopped at the row cap: the count shows as a warning with a +. */
    limited: boolean;
    tip?: string;
  }
</script>

<script lang="ts">
  import type { Snippet } from "svelte";
  import Badge from "../ui/Badge.svelte";
  import Count from "../ui/Count.svelte";
  import Spacer from "../ui/Spacer.svelte";
  import Tab from "../ui/Tab.svelte";
  import TabList from "../ui/TabList.svelte";

  /** The bar over the results: one tab per result set, then Messages; the result's tools at the end. */
  interface Props {
    tabs: ResultTabItem[];
    /** The result shown, by its index, or the messages. */
    active: number | "messages";
    errorCount: number;
    onSelect: (tab: number | "messages") => void;
    actions?: Snippet;
  }

  let { tabs, active, errorCount, onSelect, actions }: Props = $props();
</script>

<div class="pb-bar">
  <TabList label="Results">
    {#each tabs as tab, index (tab.key)}
      <Tab selected={active === index} tip={tab.tip} onclick={() => onSelect(index)}>
        {tab.label}
        {#if tab.limited}<Badge size="sm" tone="warning">{tab.count}+</Badge>{:else}<Count value={tab.count} />{/if}
      </Tab>
    {/each}
    <Tab selected={active === "messages"} onclick={() => onSelect("messages")}>
      Messages{#if errorCount > 0}<Count alert value={errorCount} />{/if}
    </Tab>
  </TabList>
  <Spacer />
  {@render actions?.()}
</div>
