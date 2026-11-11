<script lang="ts">
  import type { HTMLTextareaAttributes } from "svelte/elements";

  interface Props extends Omit<HTMLTextareaAttributes, "class" | "value"> {
    value?: string;
    mono?: boolean;
    class?: string;
    ref?: HTMLTextAreaElement;
  }

  let { value = $bindable(""), mono = false, class: extra = "", ref = $bindable(), spellcheck = false, ...rest }: Props = $props();
</script>

<textarea bind:this={ref} bind:value class={["pb-input", "pb-textarea", mono && "pb-mono", extra]} {spellcheck} {...rest}></textarea>
