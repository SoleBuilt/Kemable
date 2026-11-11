<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon, { type IconName } from "./Icon.svelte";
  import Keys from "./Keys.svelte";

  interface Props {
    icon: IconName;
    title: string;
    text?: string;
    /** Shortcuts shown under the actions. */
    keys?: { keys: string[]; label: string }[];
    /** Fits a pane instead of filling the window. */
    compact?: boolean;
    /** The actions, in one row. */
    children?: Snippet;
  }

  let { icon, title, text, keys = [], compact = false, children }: Props = $props();
</script>

<div class={["pb-empty", compact && "pb-empty--results"]}>
  <div class="pb-empty-mark"><Icon name={icon} /></div>
  <div class="pb-empty-words">
    <h2 class="pb-empty-title">{title}</h2>
    {#if text}<p class="pb-empty-text">{text}</p>{/if}
  </div>
  {#if children}<div class="pb-row">{@render children()}</div>{/if}
  {#if keys.length > 0}
    <div class="pb-empty-keys">
      {#each keys as key (key.label)}<span><Keys keys={key.keys} />{key.label}</span>{/each}
    </div>
  {/if}
</div>
