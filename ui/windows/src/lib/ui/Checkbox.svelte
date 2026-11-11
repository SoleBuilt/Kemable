<script lang="ts">
  import type { Snippet } from "svelte";
  import type { HTMLInputAttributes } from "svelte/elements";

  interface Props extends Omit<HTMLInputAttributes, "class" | "checked" | "type" | "children"> {
    checked?: boolean;
    /** The box sits by the first line of a label that wraps. */
    top?: boolean;
    class?: string;
    children?: Snippet;
  }

  let { checked = $bindable(false), top = false, class: extra = "", children, ...rest }: Props = $props();
</script>

<label class={["pb-check", top && "is-top", extra]}>
  <input type="checkbox" bind:checked {...rest} />
  {@render children?.()}
</label>
