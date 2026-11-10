<script lang="ts" module>
  import type { ObjectInfo } from "../types";

  /** What a right-click landed on, for the tree's menu (lib/sidebar/TreeMenu.svelte). */
  export type TreeTarget =
    | { kind: "object"; object: ObjectInfo; database: string | null }
    | { kind: "database"; name: string };
</script>

<script lang="ts">
  import { untrack } from "svelte";
  import { errorTitle } from "../errors";
  import { asCoreError, ipc } from "../ipc";
  import type { CoreError, ObjectKind, OpenedConnection, SchemaInfo } from "../types";
  import Icon, { type IconName } from "../ui/Icon.svelte";

  interface Props {
    connection: OpenedConnection;
    /** The database the editor runs in; its node is marked and opened. */
    database: string | null;
    /** Bumped by the parent after a run that may have changed the schema. */
    refreshKey: number;
    onOpen: (object: ObjectInfo, database: string | null) => void;
    /** Double-clicking a database makes the editor run in it. */
    onUseDatabase: (database: string) => void;
    /** A right-click: the owner shows the menu for `target` at the pointer. */
    onContext: (target: TreeTarget, x: number, y: number) => void;
  }

  let { connection, database, refreshKey, onOpen, onUseDatabase, onContext }: Props = $props();

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

  /** Enter opens the selected table, as a double-click does. */
  function onKeydown(event: KeyboardEvent, node: DatabaseNode, object: ObjectInfo) {
    if (event.key === "Enter") {
      event.preventDefault();
      onOpen(object, node.name);
    } else if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) {
      event.preventDefault();
      const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
      onContext({ kind: "object", object, database: node.name }, box.left + 24, box.bottom);
    }
  }
</script>

<div class="pb-search">
  <Icon name="search" />
  <input placeholder="Filter objects" aria-label="Filter objects" spellcheck="false" bind:value={filter} />
</div>

<div class="pb-side-body">
  <div class="pb-tree" role="tree" aria-label="Objects">
    {#if topError}
      <div class="pb-tree-error">
        <span class="pb-tree-error-title">{errorTitle(topError)}</span>
        <span>{topError.message}</span>
      </div>
    {/if}

    {#each nodes as node (node.name ?? "")}
      {@const base = node.name === null ? 0 : 1}
      {#if node.name !== null}
        {@const name = node.name}
        <button
          class="pb-tree-row"
          class:is-current={name === database}
          style:--depth="0"
          role="treeitem"
          aria-expanded={node.open}
          aria-selected={name === database}
          data-tip-start="" data-tip={name === database ? "The editor runs here" : "Double-click to run the editor here"}
          onclick={() => toggle(node)}
          ondblclick={() => onUseDatabase(name)}
          oncontextmenu={(event) => context(event, { kind: "database", name })}
        >
          <Icon name={node.open ? "chev-down" : "chev-right"} class="pb-twist" />
          <Icon name="database" class="pb-kind" />{name}
        </button>
      {/if}

      {#if node.open}
        {#if node.loading && node.groups === null}
          <div class="pb-tree-note" style:--depth={base}>Loading…</div>
        {/if}
        {#if node.error}
          <div class="pb-tree-error" style:--depth={base}>
            <span class="pb-tree-error-title">{errorTitle(node.error)}</span>
            <span>{node.error.message}</span>
          </div>
        {/if}
        {#each node.groups ?? [] as group (group.schema.name)}
          <button
            class="pb-tree-row"
            style:--depth={base}
            role="treeitem"
            aria-expanded={group.open}
            aria-selected="false"
            onclick={() => (group.open = !group.open)}
          >
            <Icon name={group.open ? "chev-down" : "chev-right"} class="pb-twist" />
            <Icon name="folder" class="pb-kind" />{group.schema.name}
          </button>
          {#if group.open}
            {#each folders(group) as folder (folder.label)}
              {@const shut = group.closed.has(folder.label) && !needle}
              <button
                class="pb-tree-row"
                style:--depth={base + 1}
                role="treeitem"
                aria-expanded={!shut}
                aria-selected="false"
                onclick={() => toggleFolder(group, folder.label)}
              >
                <Icon name={shut ? "chev-right" : "chev-down"} class="pb-twist" />{folder.label}<span class="pb-meta"
                  >{folder.objects.length}</span
                >
              </button>
              {#if !shut}
                {#each folder.objects as object (key(node, object))}
                  <button
                    class="pb-tree-row"
                    class:is-selected={selected === key(node, object)}
                    style:--depth={base + 2}
                    role="treeitem"
                    aria-selected={selected === key(node, object)}
                    onclick={() => (selected = key(node, object))}
                    ondblclick={() => onOpen(object, node.name)}
                    onkeydown={(event) => onKeydown(event, node, object)}
                    oncontextmenu={(event) => {
                      selected = key(node, object);
                      context(event, { kind: "object", object, database: node.name });
                    }}
                  >
                    <span class="pb-twist"></span><Icon name={folder.icon} class="pb-kind" />{object.name}
                  </button>
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
