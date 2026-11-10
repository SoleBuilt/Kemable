<script lang="ts">
  import { onMount, tick } from "svelte";
  import Icon, { type IconName } from "../ui/Icon.svelte";
  import type { TreeTarget } from "./ObjectTree.svelte";

  /**
   * The object tree's right-click menu, with the items a desktop database
   * client has. Items the core cannot do yet stay listed, disabled, so the menu
   * keeps its shape as they arrive.
   */
  interface Props {
    target: TreeTarget;
    x: number;
    y: number;
    onClose: () => void;
    onOpen: (target: TreeTarget & { kind: "object" }) => void;
    onUseDatabase: (name: string) => void;
    onCopy: (text: string, what: string) => void;
    onRefresh: () => void;
  }

  let { target, x, y, onClose, onOpen, onUseDatabase, onCopy, onRefresh }: Props = $props();

  interface Item {
    label: string;
    icon: IconName;
    keys?: string[];
    danger?: boolean;
    /** Null while the core cannot do it. */
    run: (() => void) | null;
  }

  const items: (Item | "-")[] = $derived.by(() => {
    if (target.kind === "database") {
      const name = target.name;
      return [
        { label: "Use as Active Database", icon: "check", run: () => onUseDatabase(name) },
        { label: "View ER Diagram", icon: "pane", run: null },
        "-",
        { label: "New Database…", icon: "plus", run: null },
        { label: "Duplicate Database…", icon: "copy", run: null },
        "-",
        { label: "New Table…", icon: "plus", run: null },
        { label: "New View…", icon: "plus", run: null },
        "-",
        { label: "Copy Name", icon: "copy", run: () => onCopy(name, "the name") },
        { label: "Refresh", icon: "refresh", keys: ["Ctrl", "R"], run: onRefresh },
      ];
    }
    const object = target.object;
    const open = { ...target };
    const qualified = object.schema ? `${object.schema}.${object.name}` : object.name;
    const table = object.kind === "table";
    const view = object.kind === "view" || object.kind === "materializedView";
    const opens = table || view;
    return [
      ...(opens
        ? [
            { label: "Open", icon: table ? "table" : "view", keys: ["Enter"], run: () => onOpen(open) } satisfies Item,
            { label: "Open in New Tab", icon: "plus", keys: ["Ctrl", "Enter"], run: () => onOpen(open) } satisfies Item,
          ]
        : []),
      { label: "Show Structure", icon: "pane", run: null },
      { label: "Show DDL", icon: "file", run: null },
      { label: "Copy DDL", icon: "copy", run: null },
      ...(view ? [{ label: "Edit View Definition", icon: "settings", run: null } satisfies Item] : []),
      "-",
      { label: "Copy Name", icon: "copy", keys: ["Ctrl", "C"], run: () => onCopy(object.name, "the name") },
      { label: "Copy Qualified Name", icon: "copy", run: () => onCopy(qualified, "the qualified name") },
      "-",
      { label: "Export…", icon: "arrow-up", run: null },
      ...(table
        ? [
            { label: "Import…", icon: "arrow-down", run: null } satisfies Item,
            { label: "Transfer To…", icon: "database", run: null } satisfies Item,
            { label: "Copy To…", icon: "database", run: null } satisfies Item,
            { label: "Generate Rows…", icon: "plus", run: null } satisfies Item,
          ]
        : []),
      "-",
      { label: "Rename…", icon: "settings", keys: ["F2"], run: null },
      ...(table
        ? [
            { label: "Edit Comment…", icon: "settings", run: null } satisfies Item,
            { label: "Maintenance…", icon: "refresh", run: null } satisfies Item,
          ]
        : []),
      "-",
      ...(table ? [{ label: "Truncate…", icon: "trash", keys: ["Alt", "Del"], danger: true, run: null } satisfies Item] : []),
      ...(opens ? [{ label: "Drop…", icon: "trash", keys: ["Del"], danger: true, run: null } satisfies Item] : []),
    ];
  });

  const title = $derived(
    target.kind === "database"
      ? `Database ${target.name}`
      : `${target.object.kind === "table" ? "Table" : target.object.kind === "view" ? "View" : target.object.kind} ${target.object.name}`,
  );

  let menu: HTMLDivElement | undefined = $state();
  let left = $state(0);
  let top = $state(0);

  // Placed at the pointer, then moved in so the whole menu stays in the window.
  onMount(() => {
    left = x;
    top = y;
    void tick().then(() => {
      const box = menu?.getBoundingClientRect();
      if (box) {
        left = Math.max(8, Math.min(x, window.innerWidth - box.width - 8));
        top = Math.max(8, Math.min(y, window.innerHeight - box.height - 8));
      }
      menu?.querySelector<HTMLElement>("button:not(:disabled)")?.focus();
    });
  });

  function choose(item: Item) {
    if (!item.run) return;
    onClose();
    item.run();
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
      return;
    }
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    const buttons = [...(menu?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? [])];
    const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
    buttons[(at + (event.key === "ArrowDown" ? 1 : -1) + buttons.length) % buttons.length]?.focus();
  }
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
<div class="pb-ctx" style:left="{left}px" style:top="{top}px" bind:this={menu} onkeydown={onKeydown}>
  <div class="pb-menu" role="menu" aria-label={title} style="min-width: 252px">
    <div class="pb-menu-label">{title}</div>
    {#each items as item, index (index)}
      {#if item === "-"}
        <div class="pb-menu-sep"></div>
      {:else}
        <button
          type="button"
          class="pb-menu-item"
          class:is-danger={item.danger}
          role="menuitem"
          disabled={item.run === null}
          onclick={() => choose(item)}
        >
          <Icon name={item.icon} />{item.label}
          {#if item.run === null}
            <span class="pb-end">Later</span>
          {:else if item.keys}
            <span class="pb-keys">{#each item.keys as key (key)}<kbd class="pb-kbd">{key}</kbd>{/each}</span>
          {/if}
        </button>
      {/if}
    {/each}
  </div>
</div>
