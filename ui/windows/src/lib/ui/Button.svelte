<script lang="ts" module>
  export type ButtonVariant = "primary" | "secondary" | "ghost" | "danger" | "link";
</script>

<script lang="ts">
  import type { Snippet } from "svelte";
  import type { HTMLButtonAttributes } from "svelte/elements";
  import Icon, { type IconName } from "./Icon.svelte";
  import { tipAttrs, type TipPlace } from "./tip";

  interface Props extends Omit<HTMLButtonAttributes, "class" | "children"> {
    variant?: ButtonVariant;
    size?: "md" | "sm";
    icon?: IconName;
    spin?: boolean;
    /** A chevron after the label, for a button that opens a menu. */
    chevron?: boolean;
    /** The name of a button that shows only its icon. */
    label?: string;
    tip?: string;
    tipAt?: TipPlace;
    /** Drawn pressed while the menu or panel it opens is open. */
    open?: boolean;
    class?: string;
    ref?: HTMLButtonElement;
    children?: Snippet;
  }

  let {
    variant = "secondary",
    size = "md",
    icon,
    spin = false,
    chevron = false,
    label,
    tip,
    tipAt = "center",
    open = false,
    class: extra = "",
    ref = $bindable(),
    type = "button",
    children,
    ...rest
  }: Props = $props();
</script>

<button
  bind:this={ref}
  {type}
  class={["pb-btn", `pb-btn--${variant}`, size === "sm" && "pb-btn--sm", !children && !chevron && "pb-btn--icon", open && "is-open", extra]}
  aria-label={children ? undefined : label}
  {...tipAttrs(tip, tipAt)}
  {...rest}
>
  {#if icon}<Icon name={icon} {spin} />{/if}{@render children?.()}{#if chevron}<Icon name="chev-down" class="pb-chev" />{/if}
</button>
