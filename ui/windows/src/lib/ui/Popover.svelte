<script lang="ts">
  import { tick, type Snippet } from "svelte";
  import { placeBy, type Placement } from "./place";

  interface Props {
    open?: boolean;
    /** Which edge of the trigger the panel lines up with, and whether it opens upward. */
    placement?: Placement;
    width?: number;
    class?: string;
    style?: string;
    /** The button that opens it; it gets the state and a toggle. */
    trigger: Snippet<[{ open: boolean; toggle: () => void }]>;
    /** The panel; it gets a function that closes it. */
    children: Snippet<[() => void]>;
  }

  let { open = $bindable(false), placement = "start", width, class: extra = "", style, trigger, children }: Props = $props();
  let anchor: HTMLDivElement | undefined = $state();
  let popStyle = $state("");

  async function toggle() {
    if (open || !anchor) {
      open = false;
      return;
    }
    popStyle = placeBy(anchor.getBoundingClientRect(), placement, { width });
    open = true;
    await tick();
    anchor.querySelector<HTMLElement>(".pb-pop [role=menuitem]:not(:disabled), .pb-pop button:not(:disabled), .pb-pop input")?.focus({ preventScroll: true });
  }

  function close() {
    open = false;
    anchor?.querySelector<HTMLElement>("[aria-haspopup]")?.focus();
  }
</script>

<!-- A panel fixed to the window would part from its trigger as the window changes size. -->
<svelte:window
  onresize={() => {
    if (open) open = false;
  }}
/>

<div bind:this={anchor} class={["pb-anchor", extra]} {style}>
  {@render trigger({ open, toggle: () => void toggle() })}
  {#if open}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="pb-clickaway" onclick={() => (open = false)}></div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="pb-pop"
      style={popStyle}
      onkeydown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          close();
        }
      }}
    >
      {@render children(close)}
    </div>
  {/if}
</div>
