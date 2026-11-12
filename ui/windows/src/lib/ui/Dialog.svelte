<script lang="ts" module>
  /** The dialogs open now, the newest last: only the top one answers Esc. */
  const open: symbol[] = [];
</script>

<script lang="ts">
  import { onDestroy, type Snippet } from "svelte";
  import Button from "./Button.svelte";

  interface Props {
    title: string;
    width?: number;
    /** The body scrolls inside the dialog when the dialog would be taller than this. */
    maxHeight?: string;
    /** While it works, Esc and the close button do nothing. */
    busy?: boolean;
    /** False for a question that must be answered with its own buttons: no close button, and Esc does nothing. */
    closable?: boolean;
    onClose: () => void;
    /** Makes the dialog a form: Enter in a field submits it. */
    onsubmit?: () => void;
    /** Extra rules for the body, such as a wider gap. */
    bodyStyle?: string;
    children: Snippet;
    /** The buttons, end-aligned; a Spacer puts some on the left. */
    footer?: Snippet;
  }

  let { title, width = 480, maxHeight = "80vh", busy = false, closable = true, onClose, onsubmit, bodyStyle, children, footer }: Props = $props();
  const id = $props.id();
  const self = Symbol();
  open.push(self);
  onDestroy(() => open.splice(open.indexOf(self), 1));

  function close() {
    if (!busy && closable) onClose();
  }
</script>

<svelte:window
  onkeydown={(event) => {
    if (event.key === "Escape" && !event.defaultPrevented && open.at(-1) === self) close();
  }}
/>

{#snippet inner()}
  <div class="pb-dialog-head">
    <h2 class="pb-dialog-title" id="{id}-title">{title}</h2>
    {#if closable}
      <Button variant="ghost" size="sm" icon="x" label="Close" tip="Close  Esc" tipAt="end" disabled={busy} onclick={close} />
    {/if}
  </div>
  <div class="pb-dialog-body is-scroll" style={bodyStyle}>
    {@render children()}
  </div>
  {#if footer}
    <div class="pb-dialog-foot">{@render footer()}</div>
  {/if}
{/snippet}

<div class="pb-overlay pb-overlay--fixed" role="presentation">
  <div class="pb-dialog" style:width="{width}px" style:max-height={maxHeight} role="dialog" aria-modal="true" aria-labelledby="{id}-title">
    {#if onsubmit}
      {@const submit = onsubmit}
      <!-- The form leaves the layout to the dialog; it only makes Enter submit. -->
      <form
        class="pb-dialog-form"
        onsubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        {@render inner()}
      </form>
    {:else}
      {@render inner()}
    {/if}
  </div>
</div>
