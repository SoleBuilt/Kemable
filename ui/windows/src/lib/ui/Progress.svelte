<script lang="ts">
  /** How far a piece of work has come: a thin bar, full and faint while its size is unknown. */
  interface Props {
    /** Done so far, 0 to 1; null when the total is unknown. */
    value: number | null;
    /** What is in progress, for a screen reader. */
    label: string;
  }

  let { value, label }: Props = $props();
  const percent = $derived(value === null ? null : Math.round(Math.min(1, Math.max(0, value)) * 100));
</script>

<div
  class={["pb-progress", percent === null && "is-unknown"]}
  role="progressbar"
  aria-label={label}
  aria-valuemin={0}
  aria-valuemax={100}
  aria-valuenow={percent ?? undefined}
>
  <div class="pb-progress-bar" style:width={percent === null ? undefined : `${percent}%`}></div>
</div>
