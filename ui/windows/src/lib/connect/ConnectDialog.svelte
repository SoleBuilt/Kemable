<script lang="ts" module>
  /** The engines this dialog connects to over the network. */
  export type NetworkEngine = "postgres" | "mssql";
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import { errorTitle } from "../errors";
  import { asCoreError, ipc } from "../ipc";
  import type { ConnectionConfig, CoreError, TlsMode } from "../types";

  interface Props {
    engine: NetworkEngine;
    /** Connects with the form's config; resolves to the error to show, or null on success. */
    onConnect: (config: ConnectionConfig) => Promise<CoreError | null>;
    onClose: () => void;
  }

  let { engine: initialEngine, onConnect, onClose }: Props = $props();

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

  onMount(() => urlInput.focus());

  async function fillFromUrl() {
    const text = url.trim();
    if (!text) return;
    try {
      const config = await ipc.parseConnectionUrl(text);
      if (config.target.kind !== "network") return;
      if (config.engine === "postgres" || config.engine === "mssql") engine = config.engine;
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

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (busy) return;
    const portNumber = port.trim() === "" ? null : Number(port);
    if (portNumber !== null && (!Number.isInteger(portNumber) || portNumber < 1 || portNumber > 65535)) {
      error = { code: "config.invalid", message: `${port} is not a port` };
      return;
    }
    busy = true;
    error = await onConnect({
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
    });
    busy = false;
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape" && !busy) onClose();
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="fixed inset-0 z-50 flex items-start justify-center bg-black/30 pt-[10vh]" role="presentation">
  <form class="w-[480px] rounded-lg border border-line bg-surface p-5 shadow-xl" onsubmit={submit} aria-labelledby="connect-title">
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
            onclick={() => (engine = id as NetworkEngine)}>{entry.title}</button
          >
        {/each}
      </div>
    </div>

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
      Paste a connection URL (Neon, Supabase, PostgreSQL or SQL Server) to fill the fields.
    </p>

    <div class="grid grid-cols-[1fr_96px] gap-x-3">
      <label class="field"><span>Host</span><input bind:value={host} required spellcheck="false" placeholder={info.hostHint} /></label>
      <label class="field"><span>Port</span><input bind:value={port} placeholder={String(info.port)} inputmode="numeric" /></label>
    </div>
    <label class="field">
      <span>Database</span>
      <input bind:value={database} placeholder="optional: every database shows in the tree" spellcheck="false" />
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
          <input bind:value={user} placeholder={engine === "postgres" ? "postgres" : "sa"} spellcheck="false" />
        </label>
        <label class="field"><span>Password</span><input bind:value={password} type="password" /></label>
      </div>
    {/if}
    <label class="field">
      <span>Encryption</span>
      <select bind:value={tls}>
        {#each TLS_MODES as mode (mode.value)}
          <option value={mode.value}>{mode.label}</option>
        {/each}
      </select>
    </label>
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
