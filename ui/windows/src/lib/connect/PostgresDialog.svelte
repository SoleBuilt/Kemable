<script lang="ts">
  import { onMount } from "svelte";
  import { errorTitle } from "../errors";
  import { asCoreError, ipc } from "../ipc";
  import type { ConnectionConfig, CoreError, TlsMode } from "../types";

  interface Props {
    /** Connects with the form's config; resolves to the error to show, or null on success. */
    onConnect: (config: ConnectionConfig) => Promise<CoreError | null>;
    onClose: () => void;
  }

  let { onConnect, onClose }: Props = $props();

  let url = $state("");
  let host = $state("localhost");
  let port = $state("");
  let database = $state("");
  let user = $state("");
  let password = $state("");
  let tls: TlsMode = $state("prefer");
  // Query parameters from a pasted URL that the form has no field for.
  let options: Record<string, string> = $state({});

  let busy = $state(false);
  let error: CoreError | null = $state(null);
  let urlInput: HTMLInputElement;

  const TLS_MODES: { value: TlsMode; label: string }[] = [
    { value: "disable", label: "Disable" },
    { value: "prefer", label: "Prefer" },
    { value: "require", label: "Require" },
    { value: "verifyFull", label: "Verify full" },
  ];

  onMount(() => urlInput.focus());

  async function fillFromUrl() {
    const text = url.trim();
    if (!text) return;
    try {
      const config = await ipc.parseConnectionUrl(text);
      if (config.target.kind !== "network") return;
      host = config.target.host;
      port = config.target.port ? String(config.target.port) : "";
      database = config.target.database ?? "";
      user = config.target.user ?? "";
      password = config.target.password ?? "";
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
      engine: "postgres",
      target: {
        kind: "network",
        host: host.trim(),
        port: portNumber,
        user: user.trim() || null,
        password,
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

<div class="fixed inset-0 z-50 flex items-start justify-center bg-black/30 pt-[12vh]" role="presentation">
  <form
    class="w-[460px] rounded-lg border border-line bg-surface p-5 shadow-xl"
    onsubmit={submit}
    aria-labelledby="pg-title"
  >
    <h2 id="pg-title" class="mb-4 text-[14px] font-semibold">Connect to PostgreSQL</h2>

    <label class="field">
      <span>URL</span>
      <input
        bind:this={urlInput}
        bind:value={url}
        onchange={fillFromUrl}
        onpaste={() => setTimeout(fillFromUrl)}
        placeholder="postgres://user:password@host:5432/database"
        spellcheck="false"
      />
    </label>
    <p class="mb-3 -mt-1 text-[11.5px] text-muted">Paste a Neon, Supabase or any PostgreSQL URL to fill the fields.</p>

    <div class="grid grid-cols-[1fr_96px] gap-x-3">
      <label class="field"><span>Host</span><input bind:value={host} required spellcheck="false" /></label>
      <label class="field"><span>Port</span><input bind:value={port} placeholder="5432" inputmode="numeric" /></label>
    </div>
    <label class="field"><span>Database</span><input bind:value={database} placeholder="same as the user" spellcheck="false" /></label>
    <div class="grid grid-cols-2 gap-x-3">
      <label class="field"><span>User</span><input bind:value={user} placeholder="postgres" spellcheck="false" /></label>
      <label class="field"><span>Password</span><input bind:value={password} type="password" /></label>
    </div>
    <label class="field">
      <span>SSL</span>
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
        <div class="mt-1 wrap-break-word whitespace-pre-wrap text-muted select-text">{error.message}</div>
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
