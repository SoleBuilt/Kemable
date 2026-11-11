<script lang="ts">
  /** A row of colour dots to pick one from: a radio group that moves with the arrow keys. */
  interface Props {
    /** The `--conn-*` token names offered, in order. */
    colors: readonly string[];
    value: string;
    onChange: (color: string) => void;
    /** The accessible name of the group. */
    label: string;
  }

  let { colors, value, onChange, label }: Props = $props();
  let group: HTMLSpanElement | undefined = $state();

  function onKeydown(event: KeyboardEvent) {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const at = colors.indexOf(value);
    const index = (at + (event.key === "ArrowRight" ? 1 : -1) + colors.length) % colors.length;
    const next = colors[index];
    if (next !== undefined) onChange(next);
    group?.querySelectorAll<HTMLButtonElement>("button")[index]?.focus();
  }
</script>

<span bind:this={group} class="pb-swatches pb-swatches--field" role="radiogroup" aria-label={label} tabindex="-1" onkeydown={onKeydown}>
  {#each colors as color (color)}
    <button
      type="button"
      class="pb-swatch"
      role="radio"
      aria-checked={color === value}
      aria-label={color}
      tabindex={color === value ? 0 : -1}
      style:--sw="var(--conn-{color})"
      onclick={() => onChange(color)}
    ></button>
  {/each}
</span>
