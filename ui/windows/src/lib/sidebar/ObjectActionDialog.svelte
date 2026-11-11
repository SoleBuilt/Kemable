<script lang="ts" module>
  /** The tree menu's actions that change the database (docs/decisions/0017-tree-actions.md). */
  export type ActionKind = "rename" | "truncate" | "drop" | "maintain" | "comment";
</script>

<script lang="ts">
  // Each action shows the statements it runs before it runs them, as edits do
  // (ADR 0010); the core writes them for the engine.
  import { onMount, tick } from "svelte";
  import { errorTitle } from "../errors";
  import { asCoreError, ipc } from "../ipc";
  import type { CoreError, Maintenance, ObjectAction, ObjectInfo } from "../types";
  import Button from "../ui/Button.svelte";
  import Checkbox from "../ui/Checkbox.svelte";
  import Combobox from "../ui/Combobox.svelte";
  import Dialog from "../ui/Dialog.svelte";
  import Field from "../ui/Field.svelte";
  import InfoBar from "../ui/InfoBar.svelte";
  import Input from "../ui/Input.svelte";
  import Spacer from "../ui/Spacer.svelte";
  import Textarea from "../ui/Textarea.svelte";

  interface Props {
    conn: number;
    database: string | null;
    /** The engine's id: only PostgreSQL drops with CASCADE. */
    engine: string;
    object: ObjectInfo;
    kind: ActionKind;
    /** It ran; `message` says what happened. */
    onDone: (message: string) => void;
    onClose: () => void;
  }

  let { conn, database, engine, object, kind, onDone, onClose }: Props = $props();

  const TASK_LABELS: Record<Maintenance, string> = {
    vacuumAnalyze: "Vacuum and analyze",
    vacuum: "Vacuum: free the space of dead rows",
    analyze: "Analyze: refresh the planner's statistics",
    reindex: "Reindex: build the indexes again",
    vacuumFull: "Vacuum full: rewrite the table (locks it)",
    vacuumDatabase: "Vacuum: rewrite the whole database file",
    updateStatistics: "Update statistics",
    reorganize: "Reorganize the indexes",
    rebuild: "Rebuild the indexes (locks the table)",
    checkTable: "Check the table (DBCC CHECKTABLE)",
  };

  const what = $derived(object.kind === "table" ? "table" : object.kind === "view" || object.kind === "materializedView" ? "view" : "object");
  // svelte-ignore state_referenced_locally
  let name = $state(object.name);
  let text = $state("");
  let cascade = $state(false);
  let tasks: Maintenance[] = $state([]);
  let task: Maintenance | null = $state(null);
  let statements: string[] = $state([]);
  let refused: CoreError | null = $state(null);
  let error: CoreError | null = $state(null);
  let busy = $state(false);
  // svelte-ignore state_referenced_locally
  let loaded = $state(kind !== "comment" && kind !== "maintain");
  let nameInput: HTMLInputElement | undefined = $state();

  onMount(async () => {
    try {
      if (kind === "comment") text = (await ipc.objectComment(conn, database, object)) ?? "";
      if (kind === "maintain") {
        tasks = await ipc.maintenanceTasks(conn);
        task = tasks[0] ?? null;
      }
    } catch (caught) {
      error = asCoreError(caught);
    }
    loaded = true;
    await tick();
    nameInput?.select();
  });

  const action: ObjectAction | null = $derived.by(() => {
    switch (kind) {
      case "rename":
        return { kind: "rename", name };
      case "truncate":
        return { kind: "truncate" };
      case "drop":
        return { kind: "drop", cascade };
      case "maintain":
        return task ? { kind: "maintain", task } : null;
      case "comment":
        return { kind: "comment", text };
    }
  });

  // The statements follow the fields as they change; the core refuses what the engine cannot do.
  let asked = 0;
  $effect(() => {
    const next = action;
    if (!loaded || !next) return;
    const call = ++asked;
    ipc.objectAction(conn, object, next).then(
      (written) => {
        if (call !== asked) return;
        statements = written;
        refused = null;
      },
      (caught: unknown) => {
        if (call !== asked) return;
        statements = [];
        refused = asCoreError(caught);
      },
    );
  });

  const title = $derived(
    kind === "rename"
      ? `Rename ${what} ${object.name}`
      : kind === "truncate"
        ? `Empty table ${object.name}?`
        : kind === "drop"
          ? `Drop ${what} ${object.name}?`
          : kind === "maintain"
            ? `Maintenance of ${object.name}`
            : `Comment on ${object.name}`,
  );
  const runLabel = $derived(
    kind === "rename" ? "Rename" : kind === "truncate" ? "Empty the table" : kind === "drop" ? "Drop" : kind === "maintain" ? "Run" : "Save comment",
  );
  const danger = $derived(kind === "truncate" || kind === "drop");

  async function run() {
    if (busy || statements.length === 0 || refused) return;
    busy = true;
    error = null;
    try {
      await ipc.runStatements(conn, database, statements);
      const done =
        kind === "rename"
          ? `Renamed ${object.name} to ${name.trim()}`
          : kind === "truncate"
            ? `Emptied ${object.name}`
            : kind === "drop"
              ? `Dropped ${object.name}`
              : kind === "maintain"
                ? `Ran ${task ? TASK_LABELS[task].split(":")[0] : "maintenance"} on ${object.name}`
                : `Saved the comment on ${object.name}`;
      onDone(done);
    } catch (caught) {
      error = asCoreError(caught);
      busy = false;
    }
  }
</script>

<Dialog {title} width={620} {busy} {onClose} onsubmit={() => void run()}>
  {#if error}<InfoBar tone="danger" title={errorTitle(error)} message={error.message} />{/if}
  {#if refused}<InfoBar tone="warning" title={errorTitle(refused)} message={refused.message} />{/if}

  {#if kind === "rename"}
    <Field label="New name"><Input bind:ref={nameInput} bind:value={name} spellcheck="false" /></Field>
  {:else if kind === "comment"}
    <Field label="Comment" hint="Empty removes the comment.">
      <Textarea bind:value={text} rows={3} placeholder="What this {what} holds" />
    </Field>
  {:else if kind === "maintain"}
    <Field label="Task" as="group">
      <Combobox
        label="Task"
        width={580}
        options={tasks.map((value) => ({ value, label: TASK_LABELS[value] }))}
        value={task}
        placeholder="Reading the tasks…"
        onPick={(picked) => (task = picked)}
      />
    </Field>
  {:else if kind === "truncate"}
    <p class="pb-dialog-text">Every row of {object.name} is deleted. This cannot be undone.</p>
  {:else if kind === "drop"}
    <p class="pb-dialog-text">The {what} {object.name} and everything in it go. This cannot be undone.</p>
    {#if engine === "postgres"}
      <Checkbox bind:checked={cascade}>Also drop what depends on it (CASCADE)</Checkbox>
    {/if}
  {/if}

  {#if statements.length > 0}
    <pre class="pb-codebox">{statements.map((statement) => `${statement};`).join("\n")}</pre>
  {/if}

  {#snippet footer()}
    <Spacer />
    <Button variant="ghost" disabled={busy} onclick={onClose}>Cancel</Button>
    <Button
      variant={danger ? "danger" : "primary"}
      type="submit"
      icon={busy ? "loader" : undefined}
      spin={busy}
      disabled={busy || statements.length === 0 || refused !== null}
      tip={`${runLabel}  Enter`}
      tipAt="end">{busy ? "Running…" : runLabel}</Button
    >
  {/snippet}
</Dialog>
