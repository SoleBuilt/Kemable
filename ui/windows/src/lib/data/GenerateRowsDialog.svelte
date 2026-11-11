<script lang="ts">
  // Generate Rows (docs/decisions/0018-export-import-generate.md): made-up rows
  // that fit each column, the first five shown before any run. A seed keeps
  // them the same, so what is shown is what goes in.
  import { errorTitle } from "../errors";
  import { asCoreError, ipc } from "../ipc";
  import type { CoreError, GeneratePlan, TableRef } from "../types";
  import Button from "../ui/Button.svelte";
  import Dialog from "../ui/Dialog.svelte";
  import Field from "../ui/Field.svelte";
  import InfoBar from "../ui/InfoBar.svelte";
  import Input from "../ui/Input.svelte";
  import Spacer from "../ui/Spacer.svelte";

  interface Props {
    conn: number;
    database: string | null;
    table: TableRef;
    onDone: (message: string) => void;
    onClose: () => void;
  }

  let { conn, database, table, onDone, onClose }: Props = $props();

  const MAX_ROWS = 1_000_000;
  let count = $state("100");
  let seed = $state(Math.floor(Math.random() * 2 ** 31));
  let plan = $state.raw<GeneratePlan | null>(null);
  let error: CoreError | null = $state(null);
  let job: number | null = $state(null);
  let rows = $state(0);
  let stopping = $state(false);
  const numberFormat = new Intl.NumberFormat();

  const wanted = $derived(Number(count.replace(/[\s,.]/g, "")));
  const valid = $derived(Number.isInteger(wanted) && wanted >= 1 && wanted <= MAX_ROWS);

  let asked = 0;
  $effect(() => {
    const current = seed;
    const call = ++asked;
    ipc.generatePlan(conn, database, table, current).then(
      (written) => {
        if (call !== asked) return;
        plan = written;
        error = null;
      },
      (caught: unknown) => {
        if (call === asked) error = asCoreError(caught);
      },
    );
  });

  async function start() {
    if (!plan || !valid || job !== null) return;
    error = null;
    rows = 0;
    try {
      job = await ipc.generateRows(conn, database, table, wanted, seed, plan.keyStart, (event) => {
        if (event.type === "progress") rows = event.rows;
        else if (event.type === "finished") onDone(`Added ${numberFormat.format(event.rows)} rows to ${table.name}`);
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

<Dialog title={`Generate rows in ${table.name}`} width={720} busy={job !== null} {onClose} onsubmit={() => void start()}>
  {#if error}<InfoBar tone="danger" title={errorTitle(error)} message={error.message} />{/if}

  {#if job !== null}
    <p class="pb-dialog-text">{stopping ? "Stopping, undoing the rows…" : `Inserted ${numberFormat.format(rows)} of ${numberFormat.format(wanted)} rows…`}</p>
  {:else}
    <Field label="Rows" hint={valid ? undefined : `A number from 1 to ${numberFormat.format(MAX_ROWS)}`}>
      <Input class="pb-num" bind:value={count} inputmode="numeric" />
    </Field>
    {#if plan}
      <p class="pb-hint">
        Fills {plan.columns.join(", ")}{plan.key ? `; ${plan.key} counts up from ${numberFormat.format(plan.keyStart)}` : ""}. Columns the server
        fills are left to it. All in one transaction: a stop undoes every row.
      </p>
      <pre class="pb-codebox">{plan.preview};</pre>
    {:else if !error}
      <p class="pb-dialog-text">Reading the table…</p>
    {/if}
  {/if}

  {#snippet footer()}
    {#if job === null}
      <Button variant="ghost" icon="refresh" tip="Other made-up values" tipAt="start" onclick={() => (seed = Math.floor(Math.random() * 2 ** 31))}>New sample</Button>
      <Spacer />
      <Button variant="ghost" onclick={onClose}>Cancel</Button>
      <Button variant="primary" type="submit" icon="plus" disabled={!plan || !valid} tip="Generate  Enter" tipAt="end"
        >Generate {valid ? numberFormat.format(wanted) : ""} rows</Button
      >
    {:else}
      <Spacer />
      <Button variant="danger" icon="stop" disabled={stopping} onclick={stop}>Stop</Button>
    {/if}
  {/snippet}
</Dialog>
