<script lang="ts">
  import type { OpenedConnection } from "../types";
  import Dot from "../ui/Dot.svelte";
  import Menu from "../ui/Menu.svelte";
  import MenuItem from "../ui/MenuItem.svelte";
  import MenuSeparator from "../ui/MenuSeparator.svelte";
  import Popover from "../ui/Popover.svelte";

  /** The title bar's connection: what is open, and a menu to edit or disconnect it (D14). */
  interface Props {
    connection: OpenedConnection;
    color?: string | null;
    /** Disconnect waits while a run is going. */
    running: boolean;
    /** Edit this connection… shows for a saved connection. */
    onEdit: (() => void) | null;
    onDisconnect: () => void;
  }

  let { connection, color = null, running, onEdit, onDisconnect }: Props = $props();
  const engine = $derived(`${connection.engine.displayName} ${connection.serverVersion ?? ""}`.trim());
</script>

<Popover width={280} style="width: 240px">
  {#snippet trigger({ open, toggle })}
    <button
      type="button"
      class="pb-input pb-select pb-combo-trigger pb-conn"
      aria-haspopup="menu"
      aria-expanded={open}
      aria-label="Connection {connection.title}"
      data-tip-start=""
      data-tip="Edit or disconnect. Ctrl+K switches"
      onclick={toggle}
    >
      <Dot {color} /><span class="pb-combo-text"><span class="pb-name">{connection.title}</span>&nbsp; <span class="pb-engine">{engine}</span></span>
    </button>
  {/snippet}
  {#snippet children(close)}
    <Menu label="Connection">
      <div class="pb-conn-card"><span><Dot {color} />{connection.title}</span><span>{engine}</span></div>
      <MenuSeparator />
      {#if onEdit}
        {@const edit = onEdit}
        <MenuItem
          icon="settings"
          onclick={() => {
            close();
            edit();
          }}>Edit this connection…</MenuItem
        >
      {/if}
      <MenuItem
        icon="unplug"
        danger
        disabled={running}
        onclick={() => {
          close();
          onDisconnect();
        }}>Disconnect</MenuItem
      >
    </Menu>
  {/snippet}
</Popover>
