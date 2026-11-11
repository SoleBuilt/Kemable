<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    label: string;
    hint?: string;
    /**
     * `label` wraps one control, so a click on the words reaches it. `group`
     * holds several (a segmented control, a host and a port).
     */
    as?: "label" | "group";
    class?: string;
    children: Snippet;
  }

  let { label, hint, as = "label", class: extra = "", children }: Props = $props();
</script>

{#if as === "label"}
  <label class={["pb-field", extra]}>
    <span class="pb-label">{label}</span>
    {@render children()}
    {#if hint}<span class="pb-hint">{hint}</span>{/if}
  </label>
{:else}
  <div class={["pb-field", extra]} role="group" aria-label={label}>
    <span class="pb-label">{label}</span>
    {@render children()}
    {#if hint}<span class="pb-hint">{hint}</span>{/if}
  </div>
{/if}
