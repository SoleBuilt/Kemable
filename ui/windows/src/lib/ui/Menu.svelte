<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    label: string;
    minWidth?: number;
    class?: string;
    ref?: HTMLDivElement;
    children: Snippet;
  }

  let { label, minWidth, class: extra = "", ref = $bindable(), children }: Props = $props();

  // The arrow keys move between the items, as a Windows menu does; Home and End go to the ends.
  function onKeydown(event: KeyboardEvent) {
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    const items = [...(ref?.querySelectorAll<HTMLButtonElement>("[role=menuitem]:not(:disabled)") ?? [])];
    if (items.length === 0) return;
    event.preventDefault();
    const at = items.indexOf(document.activeElement as HTMLButtonElement);
    const next =
      event.key === "Home" ? 0 : event.key === "End" ? items.length - 1 : (at + (event.key === "ArrowDown" ? 1 : -1) + items.length) % items.length;
    items[next]?.focus({ preventScroll: true });
  }
</script>

<div bind:this={ref} class={["pb-menu", extra]} role="menu" tabindex="-1" aria-label={label} style:min-width={minWidth ? `${minWidth}px` : undefined} onkeydown={onKeydown}>
  {@render children()}
</div>
