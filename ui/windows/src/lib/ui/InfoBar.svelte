<script lang="ts">
  import type { Snippet } from "svelte";
  import Button from "./Button.svelte";
  import Icon, { type IconName } from "./Icon.svelte";

  interface Props {
    tone?: "neutral" | "danger" | "warning" | "success";
    title: string;
    message?: string;
    /** Full width with a line under it, at the top of an area; else a rounded box. */
    strip?: boolean;
    onDismiss?: () => void;
    /** Buttons after the words. */
    children?: Snippet;
  }

  let { tone = "neutral", title, message, strip = false, onDismiss, children }: Props = $props();

  const ICONS: Record<NonNullable<Props["tone"]>, IconName> = { neutral: "info", danger: "error", warning: "warning", success: "check-circle" };
</script>

<div class={["pb-info", tone !== "neutral" && `pb-info--${tone}`, strip && "pb-info--strip"]} role={tone === "danger" ? "alert" : "status"}>
  <Icon name={ICONS[tone]} />
  <div class="pb-info-text">
    <span class="pb-info-title">{title}</span>
    {#if message}<span class="pb-info-msg">{message}</span>{/if}
  </div>
  {@render children?.()}
  {#if onDismiss}<Button variant="ghost" size="sm" icon="x" label="Dismiss" onclick={onDismiss} />{/if}
</div>
