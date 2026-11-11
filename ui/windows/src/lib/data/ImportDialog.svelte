<script lang="ts">
  // Import (docs/decisions/0018-export-import-generate.md): a CSV file into
  // this table (its fields matched to columns by name) or into a new table
  // made from it, in one transaction. What runs is shown before it runs.
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { errorTitle } from "../errors";
  import { asCoreError, ipc } from "../ipc";
  import type { CoreError, CsvPreview, ImportPlan, ImportTarget, NewColumn, TableRef } from "../types";
  import Button from "../ui/Button.svelte";
  import Checkbox from "../ui/Checkbox.svelte";
  import Combobox from "../ui/Combobox.svelte";
  import Dialog from "../ui/Dialog.svelte";
  import Field from "../ui/Field.svelte";
  import InfoBar from "../ui/InfoBar.svelte";
  import Input from "../ui/Input.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import Spacer from "../ui/Spacer.svelte";

  interface Props {
    conn: number;
    database: string | null;
    /** The table the menu was opened on; a new table goes in its schema. */
    table: TableRef;
    onDone: (message: string) => void;
    onClose: () => void;
  }

  let { conn, database, table, onDone, onClose }: Props = $props();

  let path: string | null = $state(null);
  let preview = $state.raw<CsvPreview | null>(null);
  let delimiter = $state(",");
  let header = $state(true);
  let emptyIsNull = $state(true);
  let into: "existing" | "new" = $state("existing");
  let newName = $state("");
  let columns: NewColumn[] = $state([]);
  let plan = $state.raw<ImportPlan | null>(null);
  let error: CoreError | null = $state(null);
  let job: number | null = $state(null);
  let rows = $state(0);
  let stopping = $state(false);
  const numberFormat = new Intl.NumberFormat();

  async function read(next: { delimiter: string | null; header: boolean }) {
    if (!path) return;
    try {
      const read = await ipc.previewCsv(conn, path, next.delimiter, next.header);
      preview = read;
      delimiter = read.delimiter;
      columns = read.header.map((name, index) => ({ name, typeName: read.types[index] ?? "text" }));
      error = null;
    } catch (caught) {
      error = asCoreError(caught);
    }
  }

  onMount(async () => {
    const picked = await open({
      multiple: false,
      directory: false,
      filters: [
        { name: "CSV", extensions: ["csv", "tsv", "txt"] },
        { name: "All files", extensions: ["*"] },
      ],
    });
    if (typeof picked !== "string") {
      onClose();
      return;
    }
    path = picked;
    newName = (picked.split(/[\\/]/).pop() ?? "imported").replace(/\.[^.]+$/, "");
    await read({ delimiter: null, header: true });
  });

  const target: ImportTarget = $derived(
    into === "existing" ? { kind: "existing", table } : { kind: "new", table: { schema: table.schema, name: newName.trim() }, columns },
  );

  // What runs follows the choices; the core writes it.
  let asked = 0;
  $effect(() => {
    const current = target;
    const options = { delimiter, header, emptyIsNull };
    if (!path || !preview || (current.kind === "new" && !current.table.name)) return;
    const call = ++asked;
    ipc.importPlan(conn, database, path, options, current).then(
      (written) => {
        if (call === asked) plan = written;
      },
      (caught: unknown) => {
        if (call === asked) {
          plan = null;
          error = asCoreError(caught);
        }
      },
    );
  });

  const DELIMITERS = [
    { value: ",", label: "Comma  ," },
    { value: ";", label: "Semicolon  ;" },
    { value: "\t", label: "Tab" },
    { value: "|", label: "Bar  |" },
  ];
  const INTO = $derived([
    { value: "existing" as const, label: `Into ${table.name}` },
    { value: "new" as const, label: "Into a new table" },
  ]);
  const matched = $derived(plan ? plan.mapping.filter((column) => column !== null).length : 0);

  async function start() {
    if (!path || !plan || job !== null || matched === 0) return;
    error = null;
    rows = 0;
    try {
      job = await ipc.importCsv(conn, database, path, { delimiter, header, emptyIsNull }, target, (event) => {
        if (event.type === "progress") rows = event.rows;
        else if (event.type === "finished") onDone(`Imported ${numberFormat.format(event.rows)} rows into ${target.table.name}`);
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

<Dialog title={`Import into ${into === "existing" ? table.name : "a new table"}`} width={720} busy={job !== null} {onClose} onsubmit={() => void start()}>
  {#if error}<InfoBar tone="danger" title={errorTitle(error)} message={error.message} />{/if}

  {#if job !== null}
    <p class="pb-dialog-text">{stopping ? "Stopping, undoing the rows…" : `Inserted ${numberFormat.format(rows)} rows…`}</p>
  {:else if preview && path}
    <p class="pb-hint pb-mono">{path}</p>
    <div class="pb-form-2">
      <Field label="Separator" as="group">
        <Combobox
          label="Separator"
          width={326}
          options={DELIMITERS}
          value={delimiter}
          onPick={(next) => {
            delimiter = next;
            void read({ delimiter: next, header });
          }}
        />
      </Field>
      <div class="pb-checks" style="justify-content: flex-end">
        <Checkbox
          checked={header}
          onchange={(event) => {
            header = (event.currentTarget as HTMLInputElement).checked;
            void read({ delimiter, header });
          }}>The first line names the columns</Checkbox
        >
        <Checkbox bind:checked={emptyIsNull}>An empty field is NULL</Checkbox>
      </div>
    </div>
    <Field label="Rows go" as="group">
      <Segmented label="Rows go" fill options={INTO} value={into} onChange={(next) => (into = next)} />
    </Field>

    {#if into === "new"}
      <Field label="New table"><Input bind:value={newName} spellcheck="false" /></Field>
      <div class="pb-columns" role="table" aria-label="New columns">
        <div class="pb-columns-row pb-columns-row--two is-head" role="row"><span role="columnheader">Column</span><span role="columnheader">Type</span></div>
        {#each columns as column, index (index)}
          <div class="pb-columns-row pb-columns-row--two" role="row">
            <Input size="sm" bind:value={column.name} aria-label="Name of column {index + 1}" spellcheck="false" />
            <Input size="sm" mono bind:value={column.typeName} aria-label="Type of column {index + 1}" spellcheck="false" />
          </div>
        {/each}
      </div>
    {:else if plan}
      <div class="pb-columns" role="table" aria-label="Fields and columns">
        <div class="pb-columns-row pb-columns-row--two is-head" role="row"><span role="columnheader">Field in the file</span><span role="columnheader">Column</span></div>
        {#each preview.header as field, index (index)}
          <div class="pb-columns-row pb-columns-row--two" role="row">
            <span role="cell">{field}</span>
            <span role="cell" class={plan.mapping[index] ? "" : "pb-hint"}>{plan.mapping[index] ?? "left out: no column of that name"}</span>
          </div>
        {/each}
      </div>
    {/if}

    {#if plan}
      <p class="pb-hint">
        {numberFormat.format(plan.records)} rows, {matched} of {preview.header.length} fields used, in one transaction: a stop or a failure undoes every row.
      </p>
      <pre class="pb-codebox">{[plan.create, plan.firstInsert].filter(Boolean).join(";\n\n")}{plan.firstInsert ? ";" : ""}</pre>
    {/if}
  {:else}
    <p class="pb-dialog-text">Reading the file…</p>
  {/if}

  {#snippet footer()}
    <Spacer />
    {#if job === null}
      <Button variant="ghost" onclick={onClose}>Cancel</Button>
      <Button variant="primary" type="submit" icon="arrow-down" disabled={!plan || matched === 0} tip="Import  Enter" tipAt="end"
        >Import {plan ? numberFormat.format(plan.records) : ""} rows</Button
      >
    {:else}
      <Button variant="danger" icon="stop" disabled={stopping} onclick={stop}>Stop</Button>
    {/if}
  {/snippet}
</Dialog>
