<script lang="ts">
  // Export (docs/decisions/0018-export-import-generate.md): a table's rows, or
  // the rows its page's filter keeps, written to a file as they arrive. A
  // stop deletes the half-written file.
  import { save } from "@tauri-apps/plugin-dialog";
  import { errorTitle } from "../errors";
  import { asCoreError, ipc } from "../ipc";
  import type { CoreError, ExportFormat, Filter, TableRef } from "../types";
  import Button from "../ui/Button.svelte";
  import Checkbox from "../ui/Checkbox.svelte";
  import Combobox from "../ui/Combobox.svelte";
  import Dialog from "../ui/Dialog.svelte";
  import Field from "../ui/Field.svelte";
  import InfoBar from "../ui/InfoBar.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import Spacer from "../ui/Spacer.svelte";

  interface Props {
    conn: number;
    database: string | null;
    table: TableRef;
    /** The table's key columns, so rows come in a steady order. */
    keyColumns: string[];
    /** The filter of the table's open page, when it has one. */
    filtered: { filters: Filter[]; matchAny: boolean } | null;
    onDone: (message: string) => void;
    onClose: () => void;
  }

  let { conn, database, table, keyColumns, filtered, onDone, onClose }: Props = $props();

  let format: ExportFormat = $state("csv");
  let delimiter = $state(",");
  let header = $state(true);
  let bom = $state(true);
  // svelte-ignore state_referenced_locally
  let scope: "all" | "filtered" = $state(filtered ? "filtered" : "all");
  let job: number | null = $state(null);
  let rows = $state(0);
  let stopping = $state(false);
  let error: CoreError | null = $state(null);
  const numberFormat = new Intl.NumberFormat();

  const FORMATS = [
    { value: "csv" as const, label: "CSV" },
    { value: "json" as const, label: "JSON" },
    { value: "insert" as const, label: "SQL INSERT" },
  ];
  const DELIMITERS = [
    { value: ",", label: "Comma  ," },
    { value: ";", label: "Semicolon  ;" },
    { value: "\t", label: "Tab" },
  ];
  const SCOPES = $derived([
    { value: "all" as const, label: "Every row" },
    { value: "filtered" as const, label: `Rows the filter keeps (${filtered?.filters.length ?? 0})`, disabled: !filtered },
  ]);
  const extension = $derived(format === "csv" ? "csv" : format === "json" ? "json" : "sql");

  async function start() {
    if (job !== null) return;
    const path = await save({
      defaultPath: `${table.name}.${extension}`,
      filters: [{ name: format === "csv" ? "CSV" : format === "json" ? "JSON" : "SQL", extensions: [extension] }],
    });
    if (typeof path !== "string") return;
    error = null;
    rows = 0;
    const query = {
      table,
      filters: scope === "filtered" && filtered ? filtered.filters : [],
      matchAny: scope === "filtered" && filtered ? filtered.matchAny : false,
      sort: [],
      key: keyColumns,
      offset: 0,
      limit: 0,
    };
    try {
      job = await ipc.exportTable(conn, database, query, { format, delimiter, header, bom }, path, (event) => {
        if (event.type === "progress") rows = event.rows;
        else if (event.type === "finished") onDone(`Exported ${numberFormat.format(event.rows)} rows to ${event.path ?? path}`);
        else if (event.type === "cancelled") onClose();
        else {
          error = event.error;
          job = null;
        }
      });
    } catch (caught) {
      error = asCoreError(caught);
    }
  }

  function stop() {
    if (job === null) return;
    stopping = true;
    void ipc.cancelRun(job);
  }
</script>

<Dialog title={`Export ${table.name}`} width={560} busy={job !== null} onClose={onClose} onsubmit={() => void start()}>
  {#if error}<InfoBar tone="danger" title={errorTitle(error)} message={error.message} />{/if}

  {#if job === null}
    <Field label="Format" as="group">
      <Segmented label="Format" fill options={FORMATS} value={format} onChange={(next) => (format = next)} />
    </Field>
    {#if format === "csv"}
      <div class="pb-form-2">
        <Field label="Separator" as="group">
          <Combobox label="Separator" width={248} options={DELIMITERS} value={delimiter} onPick={(next) => (delimiter = next)} />
        </Field>
        <div class="pb-checks" style="justify-content: flex-end">
          <Checkbox bind:checked={header}>Column names on the first line</Checkbox>
          <Checkbox bind:checked={bom}>UTF-8 with BOM, so Excel reads accents</Checkbox>
        </div>
      </div>
    {/if}
    <Field label="Rows" as="group" hint={filtered ? undefined : "Open the table and set a filter to export only its rows."}>
      <Segmented label="Rows" fill options={SCOPES} value={scope} onChange={(next) => (scope = next)} />
    </Field>
    <p class="pb-hint">Rows are written as the server sends them, so a large table never sits in memory. Stop deletes the file.</p>
  {:else}
    <p class="pb-dialog-text">{stopping ? "Stopping…" : `Writing ${numberFormat.format(rows)} rows…`}</p>
  {/if}

  {#snippet footer()}
    <Spacer />
    {#if job === null}
      <Button variant="ghost" onclick={onClose}>Cancel</Button>
      <Button variant="primary" type="submit" icon="arrow-up" tip="Choose the file  Enter" tipAt="end">Export…</Button>
    {:else}
      <Button variant="danger" icon="stop" disabled={stopping} onclick={stop}>Stop</Button>
    {/if}
  {/snippet}
</Dialog>
