<script lang="ts">
  import { onMount } from "svelte";
  import { appWindow } from "../ipc";

  /** Minimize, maximize or restore, and close: drawn like Windows 11's own, since the window has no system title bar. */
  let maximized = $state(false);

  onMount(() => {
    let stop: (() => void) | undefined;
    let gone = false;
    const read = () => void appWindow.isMaximized().then((value) => (maximized = value), () => {});
    read();
    void appWindow.onResized(read).then((unlisten) => {
      if (gone) unlisten();
      else stop = unlisten;
    });
    return () => {
      gone = true;
      stop?.();
    };
  });
</script>

<div class="pb-winctl" role="group" aria-label="Window">
  <button type="button" class="pb-winctl-btn" aria-label="Minimize" onclick={() => void appWindow.minimize()}>
    <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M0 5.5h10" /></svg>
  </button>
  <button type="button" class="pb-winctl-btn" aria-label={maximized ? "Restore" : "Maximize"} onclick={() => void appWindow.toggleMaximize()}>
    {#if maximized}
      <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M2.5 2.5V.5h7v7h-2" /><path d="M.5 2.5h7v7h-7z" /></svg>
    {:else}
      <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M.5 .5h9v9h-9z" /></svg>
    {/if}
  </button>
  <button type="button" class="pb-winctl-btn is-close" aria-label="Close" onclick={() => void appWindow.close()}>
    <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M0 0l10 10M10 0L0 10" /></svg>
  </button>
</div>
