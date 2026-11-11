<script lang="ts" module>
  import type { ObjectInfo } from "../types";

  /** What a right-click landed on, for the tree's menu (lib/sidebar/TreeMenu.svelte). */
  export type TreeTarget =
    | { kind: "object"; object: ObjectInfo; database: string | null }
    | { kind: "database"; name: string };

  /** Every database the login may see, or only the one the editor runs in. */
  export type TreeScope = "all" | "current";
</script>

<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { asCoreError, ipc } from "../ipc";
  import type { CoreError, ObjectKind, OpenedConnection, SchemaInfo } from "../types";
  import ErrorNote from "../ui/ErrorNote.svelte";
  import type { IconName } from "../ui/Icon.svelte";
  import SearchField from "../ui/SearchField.svelte";
  import TreeRow from "../ui/TreeRow.svelte";

  interface Props {
    connection: OpenedConnection;
    /** The database the editor runs in; its node is marked and opened. */
    database: string | null;
    /** Bumped by the parent after a run that may have changed the schema. */
    refreshKey: number;
    /** `current` shows only `database`'s schemas, without the databases above them. */
    scope?: TreeScope;
    /** Opens a table or view; `newTab` opens it on another page even when it is open (Ctrl+Enter). */
    onOpen: (object: ObjectInfo, database: string | null, newTab?: boolean) => void;
    /** Double-clicking a database makes the editor run in it. */
    onUseDatabase: (database: string) => void;
    /** A right-click: the owner shows the menu for `target` at the pointer. */
    onContext: (target: TreeTarget, x: number, y: number) => void;
    /** Ctrl+C on a row: copies `text`, which is `what`. */
    onCopy: (text: string, what: string) => void;
  }

  let { connection, database, refreshKey, scope = "all", onOpen, onUseDatabase, onContext, onCopy }: Props = $props();

  interface Folder {
    kinds: ObjectKind[];
    label: string;
    icon: IconName;
  }

  /** The kinds of a schema, in the order the tree lists them, each a folder. */
  const KINDS: Folder[] = [
    { kinds: ["table"], label: "Tables", icon: "table" },
    { kinds: ["view", "materializedView"], label: "Views", icon: "view" },
    { kinds: ["function", "tableFunction", "procedure"], label: "Functions", icon: "fn" },
    { kinds: ["sequence"], label: "Sequences", icon: "fn" },
    { kinds: ["trigger"], label: "Triggers", icon: "fn" },
    { kinds: ["index"], label: "Indexes", icon: "key" },
  ];
  /** SQL Server's folders are SSMS's. */
  const SQL_SERVER_KINDS: Folder[] = [
    { kinds: ["table"], label: "Tables", icon: "table" },
    { kinds: ["view"], label: "Views", icon: "view" },
    { kinds: ["procedure"], label: "Stored Procedures", icon: "fn" },
    { kinds: ["tableFunction"], label: "Table-valued Functions", icon: "fn" },
    { kinds: ["function"], label: "Scalar-valued Functions", icon: "fn" },
    { kinds: ["sequence"], label: "Sequences", icon: "fn" },
  ];
  const kinds = $derived(connection.engine.id === "mssql" ? SQL_SERVER_KINDS : KINDS);

  interface Group {
    schema: SchemaInfo;
    objects: ObjectInfo[];
    open: boolean;
    /** Folder labels that are closed; Tables starts open, the rest closed. */
    closed: Set<string>;
  }

  /** A database, or for an engine without databases the one unnamed root. */
  interface DatabaseNode {
    name: string | null;
    open: boolean;
    loading: boolean;
    error: CoreError | null;
    groups: Group[] | null;
  }

  let nodes: DatabaseNode[] = $state([]);
  let topError: CoreError | null = $state(null);
  let filter = $state("");
  let selected: string | null = $state(null);

  /** Reads the tree again, keeping which nodes were open. */
  export async function refresh() {
    const conn = connection.id;
    topError = null;
    const before = new Map(nodes.map((node) => [node.name, node]));
    try {
      if (connection.engine.hasDatabases) {
        const databases = await ipc.listDatabases(conn);
        nodes = databases.map((info) => ({
          name: info.name,
          open: (before.get(info.name)?.open ?? false) || info.name === database,
          loading: false,
          error: null,
          groups: null,
        }));
      } else {
        nodes = [{ name: null, open: true, loading: false, error: null, groups: null }];
      }
      await Promise.all(
        nodes.filter((node) => node.open).map((node) => loadNode(node, before.get(node.name)?.groups ?? null)),
      );
    } catch (caught) {
      topError = asCoreError(caught);
    }
  }

  async function loadNode(node: DatabaseNode, previous: Group[] | null) {
    node.loading = true;
    node.error = null;
    try {
      const schemas = await ipc.listSchemas(connection.id, node.name);
      const lists = await Promise.all(schemas.map((schema) => ipc.listObjects(connection.id, node.name, schema.name)));
      const was = new Map((previous ?? []).map((group) => [group.schema.name, group]));
      node.groups = schemas.map((schema, index) => ({
        schema,
        objects: lists[index] ?? [],
        open: was.get(schema.name)?.open ?? schema.isDefault,
        closed: was.get(schema.name)?.closed ?? new Set(kinds.slice(1).map((kind) => kind.label)),
      }));
    } catch (caught) {
      node.error = asCoreError(caught);
    } finally {
      node.loading = false;
    }
  }

  function toggle(node: DatabaseNode) {
    node.open = !node.open;
    if (node.open && node.groups === null && !node.loading) void loadNode(node, null);
  }

  function toggleFolder(group: Group, label: string) {
    const closed = new Set(group.closed);
    if (closed.has(label)) closed.delete(label);
    else closed.add(label);
    group.closed = closed;
  }

  // Reload on a new connection or a refresh request only. `refresh` reads the
  // tree it replaces, so it runs untracked, or every load would trigger another.
  $effect(() => {
    void refreshKey;
    void connection.id;
    untrack(() => void refresh());
  });

  // A database the editor moves to opens in the tree.
  $effect(() => {
    const current = database;
    untrack(() => {
      const node = nodes.find((candidate) => candidate.name === current);
      if (node && !node.open) toggle(node);
    });
  });

  const needle = $derived(filter.trim().toLowerCase());

  function folders(group: Group) {
    return kinds
      .map((kind) => ({
        ...kind,
        objects: group.objects.filter(
          (object) => kind.kinds.includes(object.kind) && (!needle || object.name.toLowerCase().includes(needle)),
        ),
      }))
      .filter((folder) => folder.objects.length > 0);
  }

  function key(node: DatabaseNode, object: ObjectInfo): string {
    return `${node.name ?? ""}.${object.schema ?? ""}.${object.kind}.${object.name}`;
  }

  // --- The rows, flat, and only those in view are drawn ---------------------
  // A database of 2,000 tables would mount 2,000 rows and take most of a
  // second; the tree draws the rows in view, as the grid draws its cells.

  type Item =
    | { type: "database"; node: DatabaseNode; name: string; depth: number }
    | { type: "schema"; group: Group; depth: number }
    | { type: "folder"; group: Group; folder: Folder; count: number; shut: boolean; depth: number }
    | { type: "object"; node: DatabaseNode; object: ObjectInfo; icon: IconName; depth: number }
    | { type: "note"; text: string; depth: number }
    | { type: "error"; error: CoreError };

  /** One row and the gap under it. */
  const PITCH = 29;
  /** An error takes three lines; its whole message is in its tip. */
  const ERROR_PITCH = 68;
  /** Rows drawn past each edge of the view, so a fast scroll never shows a gap. */
  const OVERSCAN = 12;

  const items = $derived.by(() => {
    const list: Item[] = [];
    if (topError) list.push({ type: "error", error: topError });
    const focused = scope === "current" && connection.engine.hasDatabases;
    const shown = focused ? nodes.filter((node) => node.name === database) : nodes;
    if (focused && shown.length === 0 && nodes.length > 0) list.push({ type: "note", text: "Pick a database in the toolbar.", depth: 0 });
    for (const node of shown) {
      const base = node.name === null || focused ? 0 : 1;
      if (node.name !== null && !focused) list.push({ type: "database", node, name: node.name, depth: 0 });
      if (!node.open) continue;
      if (node.loading && node.groups === null) list.push({ type: "note", text: "Loading…", depth: base });
      if (node.error) list.push({ type: "error", error: node.error });
      for (const group of node.groups ?? []) {
        list.push({ type: "schema", group, depth: base });
        if (!group.open) continue;
        const shownFolders = folders(group);
        for (const folder of shownFolders) {
          const shut = group.closed.has(folder.label) && !needle;
          list.push({ type: "folder", group, folder, count: folder.objects.length, shut, depth: base + 1 });
          if (shut) continue;
          for (const object of folder.objects) list.push({ type: "object", node, object, icon: folder.icon, depth: base + 2 });
        }
        if (shownFolders.length === 0) list.push({ type: "note", text: needle ? "Nothing matches." : "No tables yet.", depth: base + 1 });
      }
    }
    return list;
  });

  /** Where each item starts, and the height of them all. */
  const layout = $derived.by(() => {
    const tops = new Array<number>(items.length);
    let top = 0;
    items.forEach((item, index) => {
      tops[index] = top;
      top += item.type === "error" ? ERROR_PITCH : PITCH;
    });
    return { tops, height: top };
  });

  let scroller: HTMLDivElement | undefined = $state();
  let scrollTop = $state(0);
  let viewHeight = $state(600);

  onMount(() => {
    if (!scroller) return;
    const view = scroller;
    const observer = new ResizeObserver(() => (viewHeight = view.clientHeight));
    observer.observe(view);
    return () => observer.disconnect();
  });

  /** The first item whose bottom is below `y`. */
  function itemAt(y: number): number {
    const tops = layout.tops;
    let low = 0;
    let high = tops.length - 1;
    while (low < high) {
      const middle = (low + high + 1) >> 1;
      if ((tops[middle] ?? 0) <= y) low = middle;
      else high = middle - 1;
    }
    return low;
  }

  const visible = $derived.by(() => {
    if (items.length === 0) return [];
    const first = Math.max(0, itemAt(scrollTop) - OVERSCAN);
    const last = Math.min(items.length - 1, itemAt(scrollTop + viewHeight) + OVERSCAN);
    const slice: { item: Item; index: number; top: number }[] = [];
    for (let index = first; index <= last; index++) {
      const item = items[index];
      if (item) slice.push({ item, index, top: layout.tops[index] ?? 0 });
    }
    return slice;
  });

  // The widest row sets the scroll width, so a long name can be scrolled to
  // whether or not its row is drawn. Measured once per list, never per frame.
  let measure: CanvasRenderingContext2D | null = null;
  const contentWidth = $derived.by(() => {
    measure ??= document.createElement("canvas").getContext("2d");
    if (!measure || !scroller) return 0;
    const style = getComputedStyle(scroller);
    // Bold rows (selected, current) are a little wider than the text is measured.
    measure.font = `600 13px ${style.getPropertyValue("--font-sans") || "sans-serif"}`;
    let widest = 0;
    for (const item of items) {
      const text = item.type === "object" ? item.object.name : item.type === "database" ? item.name : item.type === "schema" ? item.group.schema.name : "";
      if (!text || item.type === "error" || item.type === "note") continue;
      // Padding, the twist, the icon and the gaps between them.
      const width = 4 + item.depth * 16 + 14 + 6 + 16 + 6 + measure.measureText(text).width + 12;
      if (width > widest) widest = width;
    }
    return Math.ceil(widest);
  });

  function context(event: MouseEvent, target: TreeTarget) {
    event.preventDefault();
    onContext(target, event.clientX, event.clientY);
  }

  /** Ctrl+C copies a row's name, the shortcut its menu shows. */
  function copyKey(event: KeyboardEvent, name: string): boolean {
    if (!(event.ctrlKey || event.metaKey) || event.shiftKey || event.altKey || event.key.toLowerCase() !== "c") return false;
    event.preventDefault();
    onCopy(name, "the name");
    return true;
  }

  /** Enter opens the selected table, as a double-click does, and Ctrl+Enter opens it on a new page. */
  function onKeydown(event: KeyboardEvent, node: DatabaseNode, object: ObjectInfo) {
    if (copyKey(event, object.name)) return;
    if (event.key === "Enter") {
      event.preventDefault();
      onOpen(object, node.name, event.ctrlKey || event.metaKey);
    } else if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) {
      event.preventDefault();
      const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
      onContext({ kind: "object", object, database: node.name }, box.left + 24, box.bottom);
    }
  }
</script>

<SearchField placeholder="Filter objects" bind:value={filter} />

<div class="pb-side-body" bind:this={scroller} onscroll={() => (scrollTop = scroller?.scrollTop ?? 0)}>
  <div class="pb-tree is-virtual" role="tree" aria-label="Objects" style:height="{layout.height + 8}px" style:--tree-width="{contentWidth}px">
    {#each visible as { item, index, top } (index)}
      <div class="pb-tree-slot" class:is-wide={item.type === "error"} style:top="{top + 4}px">
        {#if item.type === "database"}
          {@const name = item.name}
          {@const node = item.node}
          <TreeRow
            depth={0}
            open={node.open}
            icon="database"
            current={name === database}
            tip={name === database ? "The editor runs here" : "Double-click to run the editor here"}
            onclick={() => toggle(node)}
            ondblclick={() => onUseDatabase(name)}
            onkeydown={(event) => copyKey(event, name)}
            oncontextmenu={(event) => context(event, { kind: "database", name })}>{name}</TreeRow
          >
        {:else if item.type === "schema"}
          {@const group = item.group}
          <TreeRow depth={item.depth} open={group.open} icon="folder" onclick={() => (group.open = !group.open)}>{group.schema.name}</TreeRow>
        {:else if item.type === "folder"}
          {@const group = item.group}
          {@const label = item.folder.label}
          <TreeRow depth={item.depth} open={!item.shut} meta={item.count} onclick={() => toggleFolder(group, label)}>{label}</TreeRow>
        {:else if item.type === "object"}
          {@const node = item.node}
          {@const object = item.object}
          {@const id = key(node, object)}
          <TreeRow
            depth={item.depth}
            icon={item.icon}
            selected={selected === id}
            onclick={() => (selected = id)}
            ondblclick={() => onOpen(object, node.name)}
            onkeydown={(event) => onKeydown(event, node, object)}
            oncontextmenu={(event) => {
              selected = id;
              context(event, { kind: "object", object, database: node.name });
            }}>{object.name}</TreeRow
          >
        {:else if item.type === "note"}
          <div class="pb-tree-note" style:--depth={item.depth}>{item.text}</div>
        {:else}
          <ErrorNote error={item.error} clamp />
        {/if}
      </div>
    {/each}
  </div>
</div>
