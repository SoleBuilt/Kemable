<script lang="ts">
  interface Props {
    /** `horizontal` lies between a top and a bottom area; `vertical` between a left and a right. */
    orientation: "horizontal" | "vertical";
    /** The size of the area before the line, in px. */
    value: number;
    min: number;
    /** A number, or a function read during the drag (the window's size changes). */
    max: number | (() => number);
    /** What a double-click puts back. */
    reset?: number;
    label: string;
    style?: string;
  }

  let { orientation, value = $bindable(), min, max, reset, label, style }: Props = $props();
  let dragging = $state(false);

  const limit = (size: number) => Math.max(min, Math.min(typeof max === "function" ? max() : max, size));

  function onPointerDown(event: PointerEvent) {
    const target = event.currentTarget as HTMLElement;
    target.setPointerCapture(event.pointerId);
    const start = orientation === "horizontal" ? event.clientY : event.clientX;
    const startValue = value;
    dragging = true;
    const move = (moveEvent: PointerEvent) => {
      value = limit(startValue + (orientation === "horizontal" ? moveEvent.clientY : moveEvent.clientX) - start);
    };
    const up = () => {
      dragging = false;
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
  }

  function onKeydown(event: KeyboardEvent) {
    const back = orientation === "horizontal" ? "ArrowUp" : "ArrowLeft";
    const forward = orientation === "horizontal" ? "ArrowDown" : "ArrowRight";
    if (event.key !== back && event.key !== forward) return;
    event.preventDefault();
    value = limit(value + (event.key === forward ? 16 : -16));
  }
</script>

<!-- A separator that can be focused is a widget: the arrow keys move it. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class={[orientation === "horizontal" ? "pb-splitter" : "pb-vsplit", dragging && "is-dragging"]}
  {style}
  role="separator"
  aria-orientation={orientation}
  aria-label={label}
  aria-valuenow={value}
  aria-valuemin={min}
  tabindex="0"
  onpointerdown={onPointerDown}
  ondblclick={() => {
    if (reset !== undefined) value = reset;
  }}
  onkeydown={onKeydown}
></div>
