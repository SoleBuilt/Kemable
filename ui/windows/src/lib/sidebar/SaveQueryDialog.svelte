<script lang="ts">
  import { onMount } from "svelte";
  import { errorTitle } from "../errors";
  import { asCoreError, ipc } from "../ipc";
  import type { CoreError, SavedQuery } from "../types";
  import Button from "../ui/Button.svelte";
  import Checkbox from "../ui/Checkbox.svelte";
  import Dialog from "../ui/Dialog.svelte";
  import Field from "../ui/Field.svelte";
  import InfoBar from "../ui/InfoBar.svelte";
  import Input from "../ui/Input.svelte";
  import Spacer from "../ui/Spacer.svelte";
  import Textarea from "../ui/Textarea.svelte";

  interface Props {
    /** The query to edit, or a new one with its SQL filled in. */
    query: SavedQuery;
    /** The saved connection open now; null when it is not a saved one. */
    connectionId: string | null;
    onSaved: () => void;
    onClose: () => void;
  }

  let { query, connectionId, onSaved, onClose }: Props = $props();

  // svelte-ignore state_referenced_locally
  let name = $state(query.name);
  // svelte-ignore state_referenced_locally
  let sql = $state(query.sql);
  // svelte-ignore state_referenced_locally
  let everywhere = $state(query.connectionId === null && query.id !== "" ? true : connectionId === null);
  let error: CoreError | null = $state(null);
  let busy = $state(false);
  let nameInput: HTMLInputElement | undefined = $state();

  onMount(() => nameInput?.focus());

  async function submit() {
    busy = true;
    try {
      await ipc.saveQuery({ ...query, name: name.trim(), sql, connectionId: everywhere ? null : connectionId });
      onSaved();
      onClose();
    } catch (caught) {
      error = asCoreError(caught);
    } finally {
      busy = false;
    }
  }
</script>

<Dialog title={query.id ? "Edit saved query" : "Save query"} width={560} {busy} {onClose} onsubmit={() => void submit()}>
  <Field label="Name"><Input bind:ref={nameInput} bind:value={name} required /></Field>
  <Field label="SQL" hint="The editor's selection, or all of its text when nothing is selected.">
    <Textarea mono bind:value={sql} rows={8} />
  </Field>
  <Checkbox bind:checked={everywhere} disabled={connectionId === null}>
    Show it for every connection{#if connectionId === null}<span class="pb-meta">&nbsp;(this connection is not saved)</span>{/if}
  </Checkbox>
  {#if error}<InfoBar tone="danger" title={errorTitle(error)} message={error.message} />{/if}

  {#snippet footer()}
    <Spacer />
    <Button variant="ghost" onclick={onClose} disabled={busy}>Cancel</Button>
    <Button variant="primary" type="submit" disabled={busy || name.trim() === ""} tip="Save  Enter" tipAt="end">Save</Button>
  {/snippet}
</Dialog>
