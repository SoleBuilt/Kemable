<script lang="ts">
  import { onMount, tick, type Snippet } from "svelte";

  interface Props {
    /** The pointer, in window pixels. */
    x: number;
    y: number;
    onClose: () => void;
    children: Snippet;
  }

  let { x, y, onClose, children }: Props = $props();
  let box: HTMLDivElement | undefined = $state();
  let left = $state(0);
  let top = $state(0);

  // Placed at the pointer, then moved in so the whole menu stays in the window.
  onMount(() => {
    left = x;
    top = y;
    void tick().then(() => {
      const rect = box?.getBoundingClientRect();
      if (rect) {
        left = Math.max(8, Math.min(x, window.innerWidth - rect.width - 8));
        top = Math.max(8, Math.min(y, window.innerHeight - rect.height - 8));
      }
      box?.querySelector<HTMLElement>("[role=menuitem]:not(:disabled)")?.focus();
    });
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div
  class="pb-clickaway"
  onclick={onClose}
  oncontextmenu={(event) => {
    event.preventDefault();
    onClose();
  }}
></div>
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="pb-ctx"
  style:left="{left}px"
  style:top="{top}px"
  bind:this={box}
  onkeydown={(event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    }
  }}
>
  {@render children()}
</div>
