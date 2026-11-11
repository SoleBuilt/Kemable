<script lang="ts">
  // Show Structure (docs/decisions/0017-tree-actions.md): a table's columns as
  // the catalog has them, and the DDL the driver scripts, side by side.
  import { onMount } from "svelte";
  import { errorTitle } from "../errors";
  import { asCoreError, ipc } from "../ipc";
  import type { ColumnInfo, CoreError, ObjectInfo } from "../types";
  import Button from "../ui/Button.svelte";
  import Dialog from "../ui/Dialog.svelte";
  import InfoBar from "../ui/InfoBar.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import Spacer from "../ui/Spacer.svelte";

  interface Props {
    conn: number;
    database: string | null;
    object: ObjectInfo;
    /** Opens the DDL on a query page. */
    onEdit: (ddl: string) => void;
    onCopy: (text: string, what: string) => void;
    onClose: () => void;
  }

  let { conn, database, object, onEdit, onCopy, onClose }: Props = $props();

  // svelte-ignore state_referenced_locally
  let view: "columns" | "ddl" = $state(object.kind === "table" || object.kind === "view" ? "columns" : "ddl");
  let columns: ColumnInfo[] = $state.raw([]);
  let ddl: string | null = $state(null);
  let columnsError: CoreError | null = $state(null);
  let ddlError: CoreError | null = $state(null);

  onMount(() => {
    if (object.kind === "table" || object.kind === "view" || object.kind === "materializedView") {
      ipc.fetchColumns(conn, database, { schema: object.schema, name: object.name }).then(
        (read) => (columns = read),
        (caught: unknown) => (columnsError = asCoreError(caught)),
      );
    }
    ipc.objectDdl(conn, database, object).then(
      (read) => (ddl = read),
      (caught: unknown) => (ddlError = asCoreError(caught)),
    );
  });

  const VIEWS = [
    { value: "columns" as const, label: "Columns" },
    { value: "ddl" as const, label: "DDL" },
  ];
  const title = $derived(object.schema ? `${object.schema}.${object.name}` : object.name);
</script>

<Dialog title={`Structure of ${title}`} width={760} maxHeight="80vh" {onClose}>
  <Segmented label="Show" options={VIEWS} value={view} onChange={(next) => (view = next)} />

  {#if view === "columns"}
    {#if columnsError}
      <InfoBar tone="danger" title={errorTitle(columnsError)} message={columnsError.message} />
    {:else}
      <div class="pb-columns" role="table" aria-label="Columns">
        <div class="pb-columns-row is-head" role="row">
          <span role="columnheader">#</span><span role="columnheader">Name</span><span role="columnheader">Type</span><span role="columnheader"
            >Null</span
          ><span role="columnheader">Default</span><span role="columnheader">Key</span>
        </div>
        {#each columns as column (column.name)}
          <div class="pb-columns-row" role="row">
            <span role="cell">{column.ordinal}</span>
            <span role="cell" class="pb-columns-name">{column.name}</span>
            <span role="cell" class="pb-mono">{column.typeName}</span>
            <span role="cell">{column.nullable ? "yes" : "no"}</span>
            <span role="cell" class="pb-mono">{column.default ?? ""}</span>
            <span role="cell">{column.primaryKey !== null ? `PK ${column.primaryKey}` : ""}</span>
          </div>
        {/each}
      </div>
    {/if}
  {:else if ddlError}
    <InfoBar tone="danger" title={errorTitle(ddlError)} message={ddlError.message} />
  {:else}
    <pre class="pb-codebox">{ddl ?? "Reading…"}</pre>
  {/if}

  {#snippet footer()}
    <Button variant="ghost" icon="copy" disabled={ddl === null} tip="Copy the DDL" tipAt="start" onclick={() => ddl && onCopy(ddl, "the DDL")}>Copy DDL</Button>
    <Button variant="ghost" icon="file" disabled={ddl === null} tip="Open the DDL on a query page" onclick={() => ddl && onEdit(ddl)}>Open in editor</Button>
    <Spacer />
    <Button variant="primary" onclick={onClose}>Close</Button>
  {/snippet}
</Dialog>
