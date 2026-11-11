<script lang="ts">
  import type { Snippet } from "svelte";
  import { tipAttrs, type TipPlace } from "./tip";

  interface Props {
    selected: boolean;
    onclick: () => void;
    tip?: string;
    tipAt?: TipPlace;
    children: Snippet;
  }

  let { selected, onclick, tip, tipAt = "start", children }: Props = $props();
</script>

<button type="button" class="pb-tab" role="tab" aria-selected={selected} {onclick} {...tipAttrs(tip, tipAt)}>{@render children()}</button>
