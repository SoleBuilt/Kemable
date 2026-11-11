<script lang="ts" module>
  export interface SegmentOption<T> {
    value: T;
    label: string;
    disabled?: boolean;
  }
</script>

<script lang="ts" generics="T">
  interface Props {
    options: SegmentOption<T>[];
    value: T;
    onChange: (value: T) => void;
    /** The accessible name of the group. */
    label: string;
    /** Each option takes an equal share of the row. */
    fill?: boolean;
    class?: string;
  }

  let { options, value, onChange, label, fill = false, class: extra = "" }: Props = $props();
  let group: HTMLSpanElement | undefined = $state();

  function pick(option: SegmentOption<T> | undefined) {
    if (option && !option.disabled && option.value !== value) onChange(option.value);
  }

  // A radio group moves with the arrow keys, as Windows' does.
  function onKeydown(event: KeyboardEvent) {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const usable = options.filter((option) => !option.disabled);
    const at = usable.findIndex((option) => option.value === value);
    const next = usable[(at + (event.key === "ArrowRight" ? 1 : -1) + usable.length) % usable.length];
    pick(next);
    const index = next ? options.indexOf(next) : -1;
    group?.querySelectorAll<HTMLButtonElement>("button")[index]?.focus();
  }
</script>

<span bind:this={group} class={["pb-seg", fill && "pb-seg--fill", extra]} role="radiogroup" aria-label={label} tabindex="-1" onkeydown={onKeydown}>
  {#each options as option, index (index)}
    <button
      type="button"
      role="radio"
      aria-checked={option.value === value}
      tabindex={option.value === value ? 0 : -1}
      disabled={option.disabled}
      onclick={() => pick(option)}>{option.label}</button
    >
  {/each}
</span>
