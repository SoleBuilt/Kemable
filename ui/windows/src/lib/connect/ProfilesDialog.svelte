<script lang="ts">
  // Credential profiles (docs/decisions/0015-credential-profiles.md): the list
  // on the left, the one picked on the right, with the connections it signs in.
  import { onMount, tick } from "svelte";
  import { errorTitle } from "../errors";
  import { asCoreError, ipc } from "../ipc";
  import type { CoreError, CredentialProfile, SavedConnection } from "../types";
  import Button from "../ui/Button.svelte";
  import Dialog from "../ui/Dialog.svelte";
  import Dot from "../ui/Dot.svelte";
  import EmptyState from "../ui/EmptyState.svelte";
  import Field from "../ui/Field.svelte";
  import Icon from "../ui/Icon.svelte";
  import InfoBar from "../ui/InfoBar.svelte";
  import Input from "../ui/Input.svelte";
  import ListItem from "../ui/ListItem.svelte";
  import Spacer from "../ui/Spacer.svelte";

  interface Props {
    /** The profile to show first; the first one when null. */
    initial?: string | null;
    /** Profiles or the connections that use them changed. */
    onChanged: () => void;
    onClose: () => void;
  }

  let { initial = null, onChanged, onClose }: Props = $props();

  /** A profile as edited: `kept` is what is saved (null for a new one), `password` is blank to keep the kept one. */
  interface Draft {
    key: number;
    kept: CredentialProfile | null;
    name: string;
    user: string;
    password: string;
  }

  let drafts: Draft[] = $state([]);
  let current: number | null = $state(null);
  let connections: SavedConnection[] = $state.raw([]);
  let loaded = $state(false);
  let busy = $state(false);
  let error: CoreError | null = $state(null);
  let confirmDelete = $state(false);
  let savedNote: string | null = $state(null);
  let nextKey = 1;
  let nameInput: HTMLInputElement | undefined = $state();

  const cur = $derived(drafts.find((draft) => draft.key === current) ?? null);
  const usedBy = $derived(cur?.kept ? connections.filter((entry) => entry.profileId === cur.kept?.id) : []);

  function asDraft(profile: CredentialProfile): Draft {
    return { key: nextKey++, kept: profile, name: profile.name, user: profile.user, password: "" };
  }

  function changed(draft: Draft): boolean {
    return !draft.kept || draft.name !== draft.kept.name || draft.user !== draft.kept.user || draft.password !== "";
  }

  async function load(pick: string | null) {
    try {
      const [profiles, saved] = await Promise.all([ipc.listProfiles(), ipc.listSaved()]);
      drafts = profiles.map(asDraft);
      connections = saved;
      current = (drafts.find((draft) => draft.kept?.id === pick) ?? drafts[0])?.key ?? null;
    } catch (caught) {
      error = asCoreError(caught);
    }
    loaded = true;
  }

  onMount(() => void load(initial));

  function pick(key: number) {
    current = key;
    confirmDelete = false;
    savedNote = null;
  }

  /** `base`, or `base-2`, `base-3`… when a profile already has that name. */
  function freeName(base: string): string {
    const taken = (name: string) => drafts.some((draft) => draft.name.trim().toLowerCase() === name.toLowerCase());
    let name = base;
    for (let n = 2; taken(name); n += 1) name = `${base}-${n}`;
    return name;
  }

  async function add(from: Draft | null) {
    const draft: Draft = {
      key: nextKey++,
      kept: null,
      name: freeName(from ? `${from.name.trim()}-copy` : "new-profile"),
      user: from?.user ?? "",
      password: "",
    };
    drafts = [...drafts, draft];
    pick(draft.key);
    await tick();
    nameInput?.select();
  }

  /** Saves every profile with a change, the one shown last; stops at the first that fails and shows it. */
  async function save() {
    if (busy) return;
    const pending = drafts.filter(changed).sort((a, b) => Number(a.key === current) - Number(b.key === current));
    if (pending.length === 0) return;
    busy = true;
    error = null;
    confirmDelete = false;
    savedNote = null;
    for (const draft of pending) {
      try {
        const stored = await ipc.saveProfile(
          { id: draft.kept?.id ?? "", name: draft.name, user: draft.user },
          draft.password !== "" ? draft.password : null,
        );
        drafts = drafts.map((other) => (other.key === draft.key ? { ...asDraft(stored), key: draft.key } : other));
      } catch (caught) {
        error = asCoreError(caught);
        current = draft.key;
        busy = false;
        return;
      }
    }
    busy = false;
    const users = usedBy.length;
    savedNote =
      users === 0
        ? "The password is kept in Windows Credential Manager."
        : `The password is kept in Windows Credential Manager. ${users} ${users === 1 ? "connection signs" : "connections sign"} in with it from now on.`;
    onChanged();
  }

  async function remove() {
    const draft = cur;
    if (!draft || busy) return;
    confirmDelete = false;
    const index = drafts.indexOf(draft);
    if (draft.kept) {
      busy = true;
      try {
        await ipc.deleteProfile(draft.kept.id);
        connections = await ipc.listSaved();
        onChanged();
      } catch (caught) {
        error = asCoreError(caught);
        busy = false;
        return;
      }
      busy = false;
    }
    drafts = drafts.filter((other) => other.key !== draft.key);
    current = (drafts[Math.min(index, drafts.length - 1)] ?? null)?.key ?? null;
    savedNote = null;
  }

  const deleteText = $derived(
    usedBy.length === 0
      ? "Its password is removed from Windows Credential Manager."
      : `${usedBy.length} ${usedBy.length === 1 ? "connection uses" : "connections use"} it. They keep its user and ask for a password when they next connect.`,
  );

  const ENGINE_NAMES: Record<string, string> = { postgres: "PostgreSQL", mssql: "SQL Server" };

  function where(entry: SavedConnection): string {
    const target = entry.config.target;
    return `${ENGINE_NAMES[entry.config.engine] ?? entry.config.engine} · ${target.kind === "network" ? target.host : ""}`;
  }

  function profileMeta(draft: Draft): string {
    const users = draft.kept ? connections.filter((entry) => entry.profileId === draft.kept?.id).length : 0;
    const state = !draft.kept ? "not saved" : users === 0 ? "not used" : `used by ${users}`;
    return `${draft.user.trim() || "no user"} · ${state}`;
  }

  function onListKey(event: KeyboardEvent) {
    if (event.key === "Delete" && cur) {
      event.preventDefault();
      confirmDelete = true;
    }
  }
</script>

<Dialog title="Credential profiles" width={720} {busy} {onClose} onsubmit={() => void save()} bodyStyle="flex-direction: row; gap: 0; padding: 0; min-height: 392px">
  <div class="pb-split-list">
    {#if drafts.length > 0}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="pb-list pb-split-scroll" aria-label="Profiles" onkeydown={onListKey}>
        {#each drafts as draft (draft.key)}
          <ListItem selected={draft.key === current} onclick={() => pick(draft.key)}>
            <span class="pb-list-line"><Icon name="key" /><span class="pb-list-name">{draft.name.trim() || "Untitled"}</span></span>
            <span class="pb-meta pb-split-meta">{profileMeta(draft)}</span>
          </ListItem>
        {/each}
      </div>
    {:else if loaded}
      <EmptyState compact icon="key" title="No profiles yet" text="A profile keeps a user and a password under a name, so many connections can share them." />
    {/if}
    <div class="pb-split-foot">
      <Button variant="ghost" size="sm" icon="plus" disabled={busy} onclick={() => void add(null)}>New profile</Button>
    </div>
  </div>

  <div class="pb-split-detail">
    {#if error}<InfoBar tone="danger" title={errorTitle(error)} message={error.message} />{/if}
    {#if cur}
      {#if confirmDelete}
        <InfoBar tone="warning" title={`Delete ${cur.name.trim() || "this profile"}?`} message={deleteText}>
          <Button variant="danger" size="sm" disabled={busy} onclick={() => void remove()}>Delete</Button>
          <Button variant="ghost" size="sm" onclick={() => (confirmDelete = false)}>Keep</Button>
        </InfoBar>
      {/if}
      {#if savedNote && !changed(cur)}<InfoBar tone="success" title="Saved" message={savedNote} />{/if}
      <Field label="Name"><Input bind:ref={nameInput} bind:value={cur.name} placeholder="app-reader" spellcheck="false" /></Field>
      <div class="pb-form-2">
        <Field label="User"><Input bind:value={cur.user} placeholder="app_reader" spellcheck="false" /></Field>
        <Field label="Password">
          <Input
            type="password"
            bind:value={cur.password}
            placeholder={cur.kept ? "Kept in Windows Credential Manager" : "Kept in Windows Credential Manager once saved"}
          />
        </Field>
      </div>
      <p class="pb-hint">For PostgreSQL and SQL Server logins. The password never leaves Windows Credential Manager.</p>

      <h3 class="pb-section-title">Used by</h3>
      {#if usedBy.length > 0}
        <div class="pb-used">
          {#each usedBy as entry (entry.id)}
            <div class="pb-used-row"><Dot /><span class="pb-list-name">{entry.name}</span><span class="pb-hint">{where(entry)}</span></div>
          {/each}
        </div>
        <p class="pb-hint">A change here reaches every connection above the next time it connects.</p>
      {:else}
        <p class="pb-hint">No saved connection uses this profile. Pick it under Credentials in the connect form.</p>
      {/if}
    {:else if loaded}
      <div class="pb-split-blank"><span class="pb-hint">Make a profile to fill this in.</span></div>
    {/if}
  </div>

  {#snippet footer()}
    {#if cur}
      <Button variant="ghost" icon="trash" disabled={busy} tip="Delete profile  Del" tipAt="start" onclick={() => (confirmDelete = true)}>Delete</Button>
      <Button variant="ghost" icon="copy" disabled={busy} onclick={() => void add(cur)}>Duplicate</Button>
    {/if}
    <Spacer />
    <Button variant="ghost" disabled={busy} onclick={onClose}>Close</Button>
    <Button variant="primary" type="submit" icon={busy ? "loader" : undefined} spin={busy} disabled={busy || !drafts.some(changed)} tip="Save  Enter" tipAt="end"
      >Save</Button
    >
  {/snippet}
</Dialog>
