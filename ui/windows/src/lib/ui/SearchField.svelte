<script lang="ts">
  import type { Snippet } from "svelte";
  import type { HTMLInputAttributes } from "svelte/elements";
  import Icon from "./Icon.svelte";

  interface Props extends Omit<HTMLInputAttributes, "class" | "value"> {
    value?: string;
    /**
     * `field`: a framed box, as in the sidebar. `menu`: the first line of a
     * menu or a combobox's list. `palette`: the command palette's top line.
     */
    variant?: "field" | "menu" | "palette";
    /** The accessible name; the placeholder when not given. */
    label?: string;
    /** Typed names and patterns: the mono font. */
    mono?: boolean;
    class?: string;
    ref?: HTMLInputElement;
    /** Drawn at the end of the box: a badge, a key. */
    end?: Snippet;
  }

  let {
    value = $bindable(""),
    variant = "field",
    label,
    mono = false,
    placeholder = "Find",
    class: extra = "",
    ref = $bindable(),
    end,
    ...rest
  }: Props = $props();

  const BOX = { field: "pb-search", menu: "pb-menu-search", palette: "pb-cmdk-input" } as const;
</script>

<div class={[BOX[variant], extra]}>
  <Icon name="search" />
  <input bind:this={ref} bind:value class:pb-mono={mono} {placeholder} aria-label={label ?? placeholder} spellcheck="false" autocomplete="off" {...rest} />
  {@render end?.()}
</div>
