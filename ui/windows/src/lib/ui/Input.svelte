<script lang="ts">
  import type { HTMLInputAttributes } from "svelte/elements";
  import { tipAttrs, type TipPlace } from "./tip";

  interface Props extends Omit<HTMLInputAttributes, "class" | "value" | "size"> {
    value?: string;
    size?: "md" | "sm";
    /** Code, names and keys: the mono font. */
    mono?: boolean;
    tip?: string;
    tipAt?: TipPlace;
    class?: string;
    ref?: HTMLInputElement;
  }

  let {
    value = $bindable(""),
    size = "md",
    mono = false,
    tip,
    tipAt = "center",
    class: extra = "",
    ref = $bindable(),
    spellcheck = false,
    autocomplete = "off",
    ...rest
  }: Props = $props();
</script>

<input
  bind:this={ref}
  bind:value
  class={["pb-input", size === "sm" && "pb-input--sm", mono && "pb-mono", extra]}
  {spellcheck}
  {autocomplete}
  {...tipAttrs(tip, tipAt)}
  {...rest}
/>
