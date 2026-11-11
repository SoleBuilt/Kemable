<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon, { type IconName } from "./Icon.svelte";
  import { tipAttrs, type TipPlace } from "./tip";

  interface Props {
    tone?: "neutral" | "warning" | "danger" | "success";
    size?: "md" | "sm";
    outline?: boolean;
    /** A dot in the badge's colour before the words: a state that is on. */
    dot?: boolean;
    icon?: IconName;
    tip?: string;
    tipAt?: TipPlace;
    class?: string;
    children: Snippet;
  }

  let { tone = "neutral", size = "md", outline = false, dot = false, icon, tip, tipAt = "center", class: extra = "", children }: Props = $props();
</script>

<span
  class={["pb-badge", tone !== "neutral" && `pb-badge--${tone}`, size === "sm" && "pb-badge--sm", outline && "pb-badge--outline", extra]}
  {...tipAttrs(tip, tipAt)}
>
  {#if dot}<span class="pb-badge-dot"></span>{/if}{#if icon}<Icon name={icon} size="sm" />{/if}{@render children()}
</span>
