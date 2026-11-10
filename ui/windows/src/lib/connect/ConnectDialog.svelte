<script lang="ts" module>
  import type { ConnectionConfig } from "../types";

  /** The engines this dialog connects to over the network. */
  export type NetworkEngine = "postgres" | "mssql" | "redis" | "d1";

  export function isNetworkEngine(id: string): id is NetworkEngine {
    return id === "postgres" || id === "mssql" || id === "redis" || id === "d1";
  }

  /** What Connect asks for: a saved connection by id, or a config as typed. */
  export interface ConnectRequest {
    config: ConnectionConfig;
    /** The saved connection this is, when it was saved or opened for editing. */
    savedId: string | null;
    /** A password typed now and not kept, for a saved connection without one. */
    password: string | null;
    title: string;
  }
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import { errorTitle } from "../errors";
  import { asCoreError, ipc } from "../ipc";
  import type { CoreError, SavedConnection, TlsMode } from "../types";
  import Combobox from "../ui/Combobox.svelte";
  import Icon from "../ui/Icon.svelte";

  interface Props {
    engine: NetworkEngine;
    /** A saved connection to edit; null for a new one. */
    saved: SavedConnection | null;
    /** Connects; resolves to the error to show, or null on success. */
    onConnect: (request: ConnectRequest) => Promise<CoreError | null>;
    /** The library changed. */
    onSaved: () => void;
    onClose: () => void;
    /** A saved connection's Connect now button, in the Saved connection list. */
    onOpenSaved: (saved: SavedConnection) => void;
  }

  let { engine: initialEngine, saved: initialSaved, onConnect, onSaved, onClose, onOpenSaved }: Props = $props();

  // svelte-ignore state_referenced_locally
  let saved: SavedConnection | null = $state(initialSaved);
  let library: SavedConnection[] = $state.raw([]);
  let confirmDelete = $state(false);

  const ENGINES: Record<NetworkEngine, { title: string; port: number; urlHint: string; hostHint: string }> = {
    postgres: {
      title: "PostgreSQL",
      port: 5432,
      urlHint: "postgres://user:password@host:5432/database",
      hostHint: "localhost",
    },
    mssql: {
      title: "SQL Server",
      port: 1433,
      urlHint: "sqlserver://user:password@host:1433/database",
      hostHint: "localhost or host\\INSTANCE",
    },
    redis: {
      title: "Redis",
      port: 6379,
      urlHint: "redis://:password@host:6379/0 (rediss:// for TLS)",
      hostHint: "localhost",
    },
    d1: {
      title: "Cloudflare D1",
      port: 0,
      urlHint: "",
      hostHint: "",
    },
  };

  // svelte-ignore state_referenced_locally
  let engine: NetworkEngine = $state(initialEngine);
  let url = $state("");
  let host = $state("localhost");
  let port = $state("");
  let database = $state("");
  /** SQL Server only: log in as the signed-in Windows user. */
  let windowsLogin = $state(true);
  let user = $state("");
  let password = $state("");
  let tls: TlsMode = $state("prefer");
  // Query parameters from a pasted URL that the form has no field for.
  let options: Record<string, string> = $state({});
  /** D1: the Cloudflare account the databases belong to. */
  let account = $state("");
  let name = $state("");
  let save = $state(true);
  let savePassword = $state(true);
  /** The id once saved (or the one being edited), so a second save updates it. */
  let savedId: string | null = $state(null);

  /** Fills the form from a saved connection, or empties it for a new one; a kept password stays kept. */
  function fill(from: SavedConnection | null) {
    saved = from;
    confirmDelete = false;
    error = null;
    password = "";
    url = "";
    if (!from) {
      savedId = null;
      name = "";
      host = "localhost";
      port = "";
      database = "";
      user = "";
      account = "";
      options = {};
      savePassword = true;
      return;
    }
    fillFields(from);
  }

  function fillFields(saved: SavedConnection) {
  if (saved.config.target.kind === "network") {
    const target = saved.config.target;
    if (isNetworkEngine(saved.config.engine)) engine = saved.config.engine;
    savedId = saved.id;
    name = saved.name;
    host = target.host;
    port = target.port ? String(target.port) : "";
    database = target.database ?? "";
    user = target.user ?? "";
    windowsLogin = saved.config.engine === "mssql" && !target.user;
    tls = target.tls ?? "prefer";
    options = { ...(saved.config.options ?? {}) };
    savePassword = saved.savePassword;
  }
  if (saved.config.target.kind === "http") {
    engine = "d1";
    savedId = saved.id;
    name = saved.name;
    account = saved.config.target.endpoint;
    database = saved.config.options?.database ?? "";
    options = { ...(saved.config.options ?? {}) };
    delete options.database;
    savePassword = saved.savePassword;
  }
  }
  // svelte-ignore state_referenced_locally
  if (initialSaved) fillFields(initialSaved);

  /** A name for a connection the user did not name. */
  function defaultName(): string {
    if (engine === "d1") return `D1 ${database.trim() || account.trim().slice(0, 8)}`;
    const who = usesLogin && user.trim() ? `${user.trim()}@` : "";
    return `${who}${host.trim()}${database.trim() ? `/${database.trim()}` : ""}`;
  }

  let busy = $state(false);
  let error: CoreError | null = $state(null);
  let urlInput: HTMLInputElement | undefined = $state();
  let accountInput: HTMLInputElement | undefined = $state();

  const info = $derived(ENGINES[engine]);
  const usesLogin = $derived(engine !== "mssql" || !windowsLogin);
  /** D1 is reached through Cloudflare's API: an account and a token, no host. */
  const http = $derived(engine === "d1");
  const ready = $derived(http ? account.trim() !== "" : host.trim() !== "");

  const TLS_MODES: { value: TlsMode; label: string }[] = [
    { value: "disable", label: "Disable" },
    { value: "prefer", label: "Prefer (encrypt, do not check the certificate)" },
    { value: "require", label: "Require (encrypt, do not check the certificate)" },
    { value: "verifyFull", label: "Verify full (check the certificate and host)" },
  ];
  // Redis has no TLS negotiation: a link is plain or TLS from its first byte, and TLS checks the certificate.
  const REDIS_TLS_MODES: { value: TlsMode; label: string }[] = [
    { value: "disable", label: "Off" },
    { value: "verifyFull", label: "TLS (check the certificate and host)" },
  ];
  const tlsModes = $derived(engine === "redis" ? REDIS_TLS_MODES : TLS_MODES);

  function pickEngine(next: NetworkEngine) {
    // Another engine is a new connection: the saved one and its fields are dropped.
    if (saved && next !== engine) fill(null);
    engine = next;
    if (next === "redis" && tls !== "verifyFull") tls = "disable";
  }

  onMount(() => {
    (http ? accountInput : urlInput)?.focus();
    ipc.listSaved().then(
      (list) => (library = list.filter((entry) => isNetworkEngine(entry.config.engine))),
      () => (library = []),
    );
  });

  const engineOptions = (Object.keys(ENGINES) as NetworkEngine[]).map((id) => ({ value: id, label: ENGINES[id].title }));
  const NONE = "";

  function where(entry: SavedConnection): string {
    const target = entry.config.target;
    const kind = isNetworkEngine(entry.config.engine) ? ENGINES[entry.config.engine].title : entry.config.engine;
    return `${kind} · ${target.kind === "network" ? target.host : target.kind === "http" ? "Cloudflare API" : "file"}`;
  }

  const savedOptions = $derived([
    { value: NONE, label: "None", meta: "A new connection" },
    ...library.map((entry) => ({
      value: entry.id,
      label: entry.name,
      meta: where(entry),
      action: { icon: "plug" as const, label: `Connect to ${entry.name} now`, run: () => onOpenSaved(entry) },
    })),
  ]);

  async function remove() {
    const id = savedId;
    if (!id) return;
    try {
      await ipc.deleteSaved(id);
      library = library.filter((entry) => entry.id !== id);
      onSaved();
      fill(null);
    } catch (caught) {
      error = asCoreError(caught);
    }
  }

  async function fillFromUrl() {
    const text = url.trim();
    if (!text) return;
    try {
      const config = await ipc.parseConnectionUrl(text);
      if (config.target.kind !== "network") return;
      if (isNetworkEngine(config.engine)) engine = config.engine;
      host = config.target.host;
      port = config.target.port ? String(config.target.port) : "";
      database = config.target.database ?? "";
      user = config.target.user ?? "";
      password = config.target.password ?? "";
      windowsLogin = engine === "mssql" && user === "";
      tls = config.target.tls ?? "prefer";
      options = config.options ?? {};
      error = null;
    } catch (caught) {
      error = asCoreError(caught);
    }
  }

  function buildConfig(): ConnectionConfig | null {
    if (http) {
      const extra: Record<string, string> = { ...options };
      if (database.trim()) extra.database = database.trim();
      return {
        engine,
        target: { kind: "http", endpoint: account.trim(), token: password },
        options: extra,
      };
    }
    const portNumber = port.trim() === "" ? null : Number(port);
    if (portNumber !== null && (!Number.isInteger(portNumber) || portNumber < 1 || portNumber > 65535)) {
      error = { code: "config.invalid", message: `${port} is not a port` };
      return null;
    }
    return {
      engine,
      target: {
        kind: "network",
        host: host.trim(),
        port: portNumber,
        user: usesLogin ? user.trim() || null : null,
        password: usesLogin ? password : "",
        database: database.trim() || null,
        tls,
      },
      options,
    };
  }

  /** Saves the form to the library; resolves to whether it worked. */
  async function saveToLibrary(config: ConnectionConfig): Promise<boolean> {
    try {
      const stored = await ipc.saveConnection(
        {
          id: savedId ?? "",
          name: name.trim() || defaultName(),
          config,
          savePassword: usesLogin && savePassword,
          lastUsed: saved?.lastUsed ?? null,
        },
        usesLogin && password !== "" ? password : null,
      );
      savedId = stored.id;
      name = stored.name;
      onSaved();
      return true;
    } catch (caught) {
      error = asCoreError(caught);
      return false;
    }
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (busy) return;
    const config = buildConfig();
    if (!config) return;
    busy = true;
    if (save && !(await saveToLibrary(config))) {
      busy = false;
      return;
    }
    // A saved connection connects by id, so a password left blank uses the
    // one kept in Credential Manager.
    error = await onConnect({
      config,
      savedId: save ? savedId : null,
      password: usesLogin && password !== "" ? password : null,
      title: name.trim() || defaultName(),
    });
    busy = false;
  }

  async function saveOnly() {
    const config = buildConfig();
    if (!config || busy) return;
    busy = true;
    if (await saveToLibrary(config)) onClose();
    busy = false;
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape" && !busy) onClose();
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="pb-overlay pb-overlay--fixed" role="presentation">
  <form class="pb-dialog" style="width: 600px" onsubmit={submit} aria-labelledby="connect-title">
    <div class="pb-dialog-head">
      <h2 class="pb-dialog-title" id="connect-title">{saved ? `Edit ${saved.name}` : "New connection"}</h2>
      <button type="button" class="pb-btn pb-btn--ghost pb-btn--sm pb-btn--icon" aria-label="Close" data-tip-end="" data-tip="Close  Esc" onclick={onClose}
        ><Icon name="x" /></button
      >
    </div>
    <div class="pb-dialog-body">
      {#if error}
        <div class="pb-info pb-info--danger" role="alert">
          <Icon name="error" />
          <div class="pb-info-text"><span class="pb-info-title">{errorTitle(error)}</span><span class="pb-info-msg">{error.message}</span></div>
        </div>
      {/if}

      <div class="pb-form-2">
        <div class="pb-field">
          <span class="pb-label">Engine</span>
          <Combobox label="Engine" width={268} options={engineOptions} value={engine} onPick={pickEngine} />
        </div>
        <div class="pb-field">
          <span class="pb-label">Saved connection</span>
          <Combobox
            label="Saved connection"
            width={268}
            listWidth={320}
            placement="end"
            searchable={library.length > 6}
            searchPlaceholder="Find a connection"
            options={savedOptions}
            value={savedId ?? NONE}
            onPick={(id) => fill(library.find((entry) => entry.id === id) ?? null)}
          />
        </div>
      </div>

      {#if http}
        <div class="pb-form-2">
          <label class="pb-field"
            ><span class="pb-label">Account ID</span><input
              class="pb-input pb-mono"
              bind:this={accountInput}
              bind:value={account}
              required
              spellcheck="false"
              placeholder="32 letters and digits, from the dashboard"
            /></label
          >
          <label class="pb-field"
            ><span class="pb-label">Database</span><input
              class="pb-input pb-mono"
              bind:value={database}
              placeholder="Optional: every database shows in the tree"
              spellcheck="false"
            /></label
          >
        </div>
        <label class="pb-field"
          ><span class="pb-label">API token</span><input
            class="pb-input"
            bind:value={password}
            type="password"
            placeholder={savedId && saved?.savePassword ? "Kept in Windows Credential Manager" : "A token with D1 Read (and Edit to write)"}
          /><span class="pb-hint">Each statement is its own request and its own transaction. Local D1 databases are SQLite files: open them with Open file.</span></label
        >
      {:else}
        <label class="pb-field"
          ><span class="pb-label">Connection URL</span><input
            class="pb-input pb-mono"
            bind:this={urlInput}
            bind:value={url}
            onchange={fillFromUrl}
            onpaste={() => setTimeout(fillFromUrl)}
            placeholder={info.urlHint}
            spellcheck="false"
          /><span class="pb-hint">Paste a URL (Neon, Supabase, PostgreSQL, SQL Server or Redis) to fill the fields below.</span></label
        >
      {/if}

      <label class="pb-field"><span class="pb-label">Name</span><input class="pb-input" bind:value={name} placeholder={defaultName() || "My database"} spellcheck="false" /></label>

      {#if !http}
        <div class="pb-form-grid">
          <label class="pb-field"><span class="pb-label">Host</span><input class="pb-input" bind:value={host} required spellcheck="false" placeholder={info.hostHint} /></label>
          <label class="pb-field"><span class="pb-label">Port</span><input class="pb-input pb-num" bind:value={port} placeholder={String(info.port)} inputmode="numeric" /></label>
        </div>
        <label class="pb-field">
          <span class="pb-label">{engine === "redis" ? "Database number" : "Database"}</span>
          <input
            class="pb-input"
            bind:value={database}
            placeholder={engine === "redis" ? "0" : "Optional: pick one after connecting"}
            inputmode={engine === "redis" ? "numeric" : undefined}
            spellcheck="false"
          />
          {#if engine !== "redis"}<span class="pb-hint">The tree shows every database this login may see.</span>{/if}
        </label>
        {#if engine === "mssql"}
          <div class="pb-field">
            <span class="pb-label">Sign in with</span>
            <span class="pb-seg pb-seg--fill" role="radiogroup" aria-label="Sign in with">
              <button type="button" role="radio" aria-checked={!windowsLogin} onclick={() => (windowsLogin = false)}>SQL Server login</button>
              <button type="button" role="radio" aria-checked={windowsLogin} onclick={() => (windowsLogin = true)}>Windows account</button>
            </span>
          </div>
        {/if}
        {#if usesLogin}
          <div class="pb-form-2">
            <label class="pb-field"
              ><span class="pb-label">User</span><input
                class="pb-input"
                bind:value={user}
                placeholder={engine === "postgres" ? "postgres" : engine === "redis" ? "Optional (ACL user)" : "sa"}
                spellcheck="false"
              /></label
            >
            <label class="pb-field"
              ><span class="pb-label">Password</span><input
                class="pb-input"
                bind:value={password}
                type="password"
                placeholder={savedId && saved?.savePassword ? "Kept in Windows Credential Manager" : ""}
              /></label
            >
          </div>
        {/if}
        <div class="pb-field">
          <span class="pb-label">Encryption</span>
          <Combobox label="Encryption" width={552} options={tlsModes} value={tls} onPick={(mode) => (tls = mode)} />
        </div>
      {/if}

      <div class="pb-checks">
        <label class="pb-check"><input type="checkbox" bind:checked={save} /> Save this connection</label>
        {#if save && usesLogin}
          <label class="pb-check" style="padding-left: 24px"
            ><input type="checkbox" bind:checked={savePassword} /> Keep the {http ? "token" : "password"} in Windows Credential Manager</label
          >
        {/if}
      </div>
      {#if Object.keys(options).length > 0}
        <p class="pb-hint">
          From the URL: {Object.entries(options)
            .map(([key, value]) => `${key}=${value}`)
            .join(", ")}
        </p>
      {/if}
    </div>
    <div class="pb-dialog-foot">
      {#if savedId}
        {#if confirmDelete}
          <span class="pb-meta">Delete {name || "it"}?</span>
          <button type="button" class="pb-btn pb-btn--danger" onclick={remove} disabled={busy}>Delete</button>
          <button type="button" class="pb-btn pb-btn--ghost" onclick={() => (confirmDelete = false)}>Keep</button>
        {:else}
          <button type="button" class="pb-btn pb-btn--ghost" onclick={() => (confirmDelete = true)} disabled={busy} data-tip-start="" data-tip="Delete this saved connection"
            ><Icon name="trash" />Delete…</button
          >
        {/if}
      {/if}
      <span class="pb-spacer"></span>
      <button type="button" class="pb-btn pb-btn--ghost" onclick={onClose} disabled={busy}>Cancel</button>
      {#if save}
        <button type="button" class="pb-btn pb-btn--secondary" onclick={saveOnly} disabled={busy || !ready}>Save</button>
      {/if}
      <button type="submit" class="pb-btn pb-btn--primary" disabled={busy || !ready} data-tip-end="" data-tip="Connect  Enter">
        {#if busy}<Icon name="loader" spin />Connecting…{:else}Connect{/if}
      </button>
    </div>
  </form>
</div>
