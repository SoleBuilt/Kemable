<script lang="ts">
  import ContextMenu from "../ui/ContextMenu.svelte";
  import type { IconName } from "../ui/Icon.svelte";
  import Menu from "../ui/Menu.svelte";
  import MenuItem from "../ui/MenuItem.svelte";
  import MenuLabel from "../ui/MenuLabel.svelte";
  import MenuSeparator from "../ui/MenuSeparator.svelte";
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
    /** Opens it on another page, even when it is open already. */
    onOpenNew: (target: TreeTarget & { kind: "object" }) => void;
    onUseDatabase: (name: string) => void;
    onCopy: (text: string, what: string) => void;
    onRefresh: () => void;
  }

  let { target, x, y, onClose, onOpen, onOpenNew, onUseDatabase, onCopy, onRefresh }: Props = $props();

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
            { label: "Open in New Tab", icon: "plus", keys: ["Ctrl", "Enter"], run: () => onOpenNew(open) } satisfies Item,
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

  function choose(item: Item) {
    if (!item.run) return;
    onClose();
    item.run();
  }
</script>

<ContextMenu {x} {y} {onClose}>
  <Menu label={title} minWidth={252}>
    <MenuLabel>{title}</MenuLabel>
    {#each items as item, index (index)}
      {#if item === "-"}
        <MenuSeparator />
      {:else}
        <MenuItem icon={item.icon} keys={item.keys} end={item.run === null ? "Later" : undefined} danger={item.danger} disabled={item.run === null} onclick={() => choose(item)}
          >{item.label}</MenuItem
        >
      {/if}
    {/each}
  </Menu>
</ContextMenu>
