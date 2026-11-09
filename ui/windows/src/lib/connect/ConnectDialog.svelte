<script lang="ts" module>
  import type { ConnectionConfig } from "../types";

  /** The engines this dialog connects to over the network. */
  export type NetworkEngine = "postgres" | "mssql" | "redis";

  export function isNetworkEngine(id: string): id is NetworkEngine {
    return id === "postgres" || id === "mssql" || id === "redis";
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

  interface Props {
    engine: NetworkEngine;
    /** A saved connection to edit; null for a new one. */
    saved: SavedConnection | null;
    /** Connects; resolves to the error to show, or null on success. */
    onConnect: (request: ConnectRequest) => Promise<CoreError | null>;
    /** The library changed. */
    onSaved: () => void;
    onClose: () => void;
  }

  let { engine: initialEngine, saved, onConnect, onSaved, onClose }: Props = $props();

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
  let name = $state("");
  let save = $state(true);
  let savePassword = $state(true);
  /** The id once saved (or the one being edited), so a second save updates it. */
  let savedId: string | null = $state(null);

  // Editing a saved connection fills the form; its kept password stays kept.
  // svelte-ignore state_referenced_locally
  if (saved && saved.config.target.kind === "network") {
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

  /** A name for a connection the user did not name. */
  function defaultName(): string {
    const who = usesLogin && user.trim() ? `${user.trim()}@` : "";
    return `${who}${host.trim()}${database.trim() ? `/${database.trim()}` : ""}`;
  }

  let busy = $state(false);
  let error: CoreError | null = $state(null);
  let urlInput: HTMLInputElement;

  const info = $derived(ENGINES[engine]);
  const usesLogin = $derived(engine !== "mssql" || !windowsLogin);

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
    engine = next;
    if (next === "redis" && tls !== "verifyFull") tls = "disable";
  }

  onMount(() => urlInput.focus());

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

<div class="fixed inset-0 z-50 flex items-start justify-center bg-black/30 pt-[10vh]" role="presentation">
  <form class="w-120 rounded-lg border border-line bg-surface p-5 shadow-xl" onsubmit={submit} aria-labelledby="connect-title">
    <div class="mb-4 flex items-center gap-3">
      <h2 id="connect-title" class="text-[14px] font-semibold">Connect to</h2>
      <div class="flex rounded-md border border-line p-0.5 text-[12px]" role="radiogroup" aria-label="Database engine">
        {#each Object.entries(ENGINES) as [id, entry] (id)}
          <button
            type="button"
            class="rounded px-2.5 py-0.5"
            class:engine-active={engine === id}
            role="radio"
            aria-checked={engine === id}
            onclick={() => pickEngine(id as NetworkEngine)}>{entry.title}</button
          >
        {/each}
      </div>
    </div>

    <label class="field">
      <span>Name</span>
      <input bind:value={name} placeholder={defaultName() || "My database"} spellcheck="false" />
    </label>

    <label class="field">
      <span>URL</span>
      <input
        bind:this={urlInput}
        bind:value={url}
        onchange={fillFromUrl}
        onpaste={() => setTimeout(fillFromUrl)}
        placeholder={info.urlHint}
        spellcheck="false"
      />
    </label>
    <p class="mb-3 -mt-1 text-[11.5px] text-muted">
      Paste a connection URL (Neon, Supabase, PostgreSQL, SQL Server or Redis) to fill the fields.
    </p>

    <div class="grid grid-cols-[1fr_96px] gap-x-3">
      <label class="field"><span>Host</span><input bind:value={host} required spellcheck="false" placeholder={info.hostHint} /></label>
      <label class="field"><span>Port</span><input bind:value={port} placeholder={String(info.port)} inputmode="numeric" /></label>
    </div>
    <label class="field">
      <span>{engine === "redis" ? "Database number" : "Database"}</span>
      <input
        bind:value={database}
        placeholder={engine === "redis" ? "0" : "optional: every database shows in the tree"}
        inputmode={engine === "redis" ? "numeric" : undefined}
        spellcheck="false"
      />
    </label>

    {#if engine === "mssql"}
      <label class="field">
        <span>Authentication</span>
        <select bind:value={windowsLogin}>
          <option value={true}>Windows Authentication (the signed-in Windows user)</option>
          <option value={false}>SQL Server login</option>
        </select>
      </label>
    {/if}
    {#if usesLogin}
      <div class="grid grid-cols-2 gap-x-3">
        <label class="field">
          <span>User</span>
          <input bind:value={user} placeholder={engine === "postgres" ? "postgres" : engine === "redis" ? "optional (ACL user)" : "sa"} spellcheck="false" />
        </label>
        <label class="field">
          <span>Password</span>
          <input
            bind:value={password}
            type="password"
            placeholder={savedId && saved?.savePassword ? "kept in Credential Manager" : ""}
          />
        </label>
      </div>
    {/if}
    <label class="field">
      <span>Encryption</span>
      <select bind:value={tls}>
        {#each tlsModes as mode (mode.value)}
          <option value={mode.value}>{mode.label}</option>
        {/each}
      </select>
    </label>
    <div class="mb-3 flex flex-col gap-1.5 text-[12px]">
      <label class="flex items-center gap-2"><input type="checkbox" bind:checked={save} /> Save this connection</label>
      {#if save && usesLogin}
        <label class="flex items-center gap-2 pl-5">
          <input type="checkbox" bind:checked={savePassword} /> Keep the password in Windows Credential Manager
        </label>
      {/if}
    </div>
    {#if Object.keys(options).length > 0}
      <p class="mb-3 text-[11.5px] text-muted">
        From the URL: {Object.entries(options)
          .map(([key, value]) => `${key}=${value}`)
          .join(", ")}
      </p>
    {/if}

    {#if error}
      <div class="mb-3 rounded border border-line bg-surface-2 p-2 text-[12px]">
        <div class="font-semibold text-danger">{errorTitle(error)}</div>
        <div class="mt-1 whitespace-pre-wrap wrap-break-word text-muted select-text">{error.message}</div>
      </div>
    {/if}

    <div class="mt-4 flex justify-end gap-2">
      <button type="button" class="dialog-button" onclick={onClose} disabled={busy}>Cancel</button>
      {#if save}
        <button type="button" class="dialog-button" onclick={saveOnly} disabled={busy || host.trim() === ""}>
          Save
        </button>
      {/if}
      <button type="submit" class="dialog-button primary" disabled={busy || host.trim() === ""}>
        {busy ? "Connecting…" : "Connect"}
      </button>
    </div>
  </form>
</div>

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-bottom: 12px;
    font-size: 12px;
  }
  .field > span {
    color: var(--text-muted);
  }
  .field input,
  .field select {
    height: 28px;
    padding: 0 8px;
    border: 1px solid var(--border);
    border-radius: 5px;
    background: var(--surface);
    color: var(--text);
    outline: none;
    user-select: text;
  }
  .field input:focus,
  .field select:focus {
    border-color: var(--accent);
  }
  .engine-active {
    background: var(--accent);
    color: var(--accent-text);
  }
  .dialog-button {
    height: 28px;
    padding: 0 14px;
    border: 1px solid var(--border);
    border-radius: 5px;
    background: var(--surface-2);
    color: var(--text);
    font-size: 12px;
  }
  .dialog-button.primary {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--accent-text);
  }
  .dialog-button:disabled {
    opacity: 0.6;
  }
</style>
