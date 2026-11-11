<script lang="ts" module>
  import type { ObjectInfo } from "../types";

  /** What a right-click landed on, for the tree's menu (lib/sidebar/TreeMenu.svelte). */
  export type TreeTarget =
    | { kind: "object"; object: ObjectInfo; database: string | null }
    | { kind: "database"; name: string };
</script>

<script lang="ts">
  import { untrack } from "svelte";
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
    /** Opens a table or view; `newTab` opens it on another page even when it is open (Ctrl+Enter). */
    onOpen: (object: ObjectInfo, database: string | null, newTab?: boolean) => void;
    /** Double-clicking a database makes the editor run in it. */
    onUseDatabase: (database: string) => void;
    /** A right-click: the owner shows the menu for `target` at the pointer. */
    onContext: (target: TreeTarget, x: number, y: number) => void;
    /** Ctrl+C on a row: copies `text`, which is `what`. */
    onCopy: (text: string, what: string) => void;
  }

  let { connection, database, refreshKey, onOpen, onUseDatabase, onContext, onCopy }: Props = $props();

  /** The kinds of a schema, in the order the tree lists them, each a folder. */
  const KINDS: { kinds: ObjectKind[]; label: string; icon: IconName }[] = [
    { kinds: ["table"], label: "Tables", icon: "table" },
    { kinds: ["view", "materializedView"], label: "Views", icon: "view" },
    { kinds: ["function", "procedure"], label: "Functions", icon: "fn" },
    { kinds: ["sequence"], label: "Sequences", icon: "fn" },
    { kinds: ["trigger"], label: "Triggers", icon: "fn" },
    { kinds: ["index"], label: "Indexes", icon: "key" },
  ];

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
        closed: was.get(schema.name)?.closed ?? new Set(KINDS.slice(1).map((kind) => kind.label)),
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
    return KINDS.map((kind) => ({
      ...kind,
      objects: group.objects.filter(
        (object) => kind.kinds.includes(object.kind) && (!needle || object.name.toLowerCase().includes(needle)),
      ),
    })).filter((folder) => folder.objects.length > 0);
  }

  function key(node: DatabaseNode, object: ObjectInfo): string {
    return `${node.name ?? ""}.${object.schema ?? ""}.${object.kind}.${object.name}`;
  }

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

<div class="pb-side-body">
  <div class="pb-tree" role="tree" aria-label="Objects">
    {#if topError}<ErrorNote error={topError} />{/if}

    {#each nodes as node (node.name ?? "")}
      {@const base = node.name === null ? 0 : 1}
      {#if node.name !== null}
        {@const name = node.name}
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
      {/if}

      {#if node.open}
        {#if node.loading && node.groups === null}
          <div class="pb-tree-note" style:--depth={base}>Loading…</div>
        {/if}
        {#if node.error}<ErrorNote error={node.error} />{/if}
        {#each node.groups ?? [] as group (group.schema.name)}
          <TreeRow depth={base} open={group.open} icon="folder" onclick={() => (group.open = !group.open)}>{group.schema.name}</TreeRow>
          {#if group.open}
            {#each folders(group) as folder (folder.label)}
              {@const shut = group.closed.has(folder.label) && !needle}
              <TreeRow depth={base + 1} open={!shut} meta={folder.objects.length} onclick={() => toggleFolder(group, folder.label)}>{folder.label}</TreeRow>
              {#if !shut}
                {#each folder.objects as object (key(node, object))}
                  <TreeRow
                    depth={base + 2}
                    icon={folder.icon}
                    selected={selected === key(node, object)}
                    onclick={() => (selected = key(node, object))}
                    ondblclick={() => onOpen(object, node.name)}
                    onkeydown={(event) => onKeydown(event, node, object)}
                    oncontextmenu={(event) => {
                      selected = key(node, object);
                      context(event, { kind: "object", object, database: node.name });
                    }}>{object.name}</TreeRow
                  >
                {/each}
              {/if}
            {/each}
            {#if folders(group).length === 0}
              <div class="pb-tree-note" style:--depth={base + 1}>{needle ? "Nothing matches." : "No tables yet."}</div>
            {/if}
          {/if}
        {/each}
      {/if}
    {/each}
  </div>
</div>
