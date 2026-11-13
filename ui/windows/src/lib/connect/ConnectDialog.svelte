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
    /** The credential profile it signs in with, for a connection not saved. */
    profileId: string | null;
    title: string;
    /** Save this connection too, once it connects (and only then), with the password typed now. */
    keep: { connection: SavedConnection; password: string | null } | null;
  }
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { errorTitle } from "../errors";
  import { asCoreError, ipc } from "../ipc";
  import type { CoreError, CredentialProfile, SavedConnection, SshTunnel, TlsMode } from "../types";
  import Combobox, { type ComboOption } from "../ui/Combobox.svelte";
  import Button from "../ui/Button.svelte";
  import Checkbox from "../ui/Checkbox.svelte";
  import Confirm from "../ui/Confirm.svelte";
  import Dialog from "../ui/Dialog.svelte";
  import Field from "../ui/Field.svelte";
  import InfoBar from "../ui/InfoBar.svelte";
  import Input from "../ui/Input.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import Spacer from "../ui/Spacer.svelte";
  import Swatches from "../ui/Swatches.svelte";

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
    /** Opens Credential profiles over this dialog, showing `profileId` first. */
    onManageProfiles: (profileId: string | null) => void;
    /** Goes up when the profiles change, to read them again. */
    profilesKey: number;
  }

  let {
    engine: initialEngine,
    saved: initialSaved,
    onConnect,
    onSaved,
    onClose,
    onOpenSaved,
    onManageProfiles,
    profilesKey,
  }: Props = $props();

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
  /** The colour of its dot in Ctrl+K and the saved list (design system D8). */
  let color = $state("gray");
  let save = $state(true);
  let savePassword = $state(true);
  /** The id once saved (or the one being edited), so a second save updates it. */
  let savedId: string | null = $state(null);
  /** Sign in with a credential profile instead of a user and password typed here. */
  let useProfile = $state(false);
  let profileId: string | null = $state(null);
  let profiles: CredentialProfile[] = $state.raw([]);
  /** The profile "Save these as a profile" just made, to say so. */
  let madeProfile: string | null = $state(null);

  // --- SSH tunnel (docs/decisions/0024-ssh-tunnels.md) ---
  let sshOn = $state(false);
  let sshHost = $state("");
  let sshPort = $state("");
  let sshUser = $state("");
  let sshWithKey = $state(false);
  let sshPassword = $state("");
  let sshKeyPath = $state("");
  let sshPassphrase = $state("");
  /** Whether the connection being edited has an SSH secret kept, so a blank field keeps it. */
  let sshKept = $state(false);

  function fillSsh(ssh: SshTunnel | null | undefined) {
    sshOn = Boolean(ssh);
    sshHost = ssh?.host ?? "";
    sshPort = ssh?.port ? String(ssh.port) : "";
    sshUser = ssh?.user ?? "";
    sshWithKey = ssh?.auth.kind === "key";
    sshKeyPath = ssh?.auth.kind === "key" ? ssh.auth.path : "";
    sshPassword = "";
    sshPassphrase = "";
    sshKept = Boolean(ssh);
  }

  /** The tunnel as typed, or null when it is off; sets the error when a field is wrong. */
  function buildSsh(): SshTunnel | null | false {
    if (!sshOn) return null;
    const portNumber = sshPort.trim() === "" ? null : Number(sshPort);
    if (portNumber !== null && (!Number.isInteger(portNumber) || portNumber < 1 || portNumber > 65535)) {
      error = { code: "config.invalid", message: `${sshPort} is not an SSH port` };
      return false;
    }
    return {
      host: sshHost.trim(),
      port: portNumber,
      user: sshUser.trim(),
      auth: sshWithKey ? { kind: "key", path: sshKeyPath.trim(), passphrase: sshPassphrase } : { kind: "password", password: sshPassword },
    };
  }

  async function pickKey() {
    const path = await open({ multiple: false, directory: false, title: "An SSH private key" });
    if (typeof path === "string") sshKeyPath = path;
  }

  const SSH_LOGINS = [
    { value: false, label: "Password" },
    { value: true, label: "Private key" },
  ];

  /** An SSH server not known yet, shown with its fingerprint: trusting it does again what was asked. */
  let unknownHost: { host: string; port: number | null; fingerprint: string; again: () => void } | null = $state(null);

  /** Keeps what to do again when `failure` is an SSH server not known yet. */
  function askTrust(failure: CoreError | null, again: () => void): CoreError | null {
    if (failure?.code !== "ssh.host_unknown") return failure;
    const portNumber = sshPort.trim() === "" ? null : Number(sshPort);
    unknownHost = { host: sshHost.trim(), port: portNumber, fingerprint: failure.message, again };
    return null;
  }

  async function trustHost() {
    const target = unknownHost;
    if (!target) return;
    unknownHost = null;
    try {
      await ipc.trustSshHost(target.host, target.port);
      target.again();
    } catch (caught) {
      error = asCoreError(caught);
    }
  }

  /** Fills the form from a saved connection, or empties it for a new one; a kept password stays kept. */
  function fill(from: SavedConnection | null) {
    saved = from;
    confirmDelete = false;
    error = null;
    password = "";
    url = "";
    madeProfile = null;
    useProfile = from?.profileId != null;
    profileId = from?.profileId ?? null;
    color = from?.color ?? "gray";
    unknownHost = null;
    fillSsh(from?.config.ssh);
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
  if (initialSaved) {
    fillFields(initialSaved);
    fillSsh(initialSaved.config.ssh);
    useProfile = initialSaved.profileId != null;
    profileId = initialSaved.profileId;
    color = initialSaved.color ?? "gray";
  }

  /** A name for a connection the user did not name. */
  function defaultName(): string {
    if (engine === "d1") return `D1 ${database.trim() || account.trim().slice(0, 8)}`;
    const login = withProfile ? (profile?.user ?? "") : user.trim();
    const who = usesLogin && login ? `${login}@` : "";
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
  /** Credential profiles hold SQL logins: PostgreSQL, and SQL Server signing in with a login. */
  const profileEngine = $derived((engine === "postgres" || engine === "mssql") && usesLogin);
  const withProfile = $derived(profileEngine && useProfile);
  const profile = $derived(profiles.find((entry) => entry.id === profileId) ?? null);
  const sshReady = $derived(!sshOn || (sshHost.trim() !== "" && sshUser.trim() !== "" && (!sshWithKey || sshKeyPath.trim() !== "")));
  const ready = $derived((http ? account.trim() !== "" : host.trim() !== "") && (!withProfile || profile !== null) && sshReady);

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

  async function loadProfiles() {
    try {
      profiles = await ipc.listProfiles();
    } catch {
      profiles = [];
    }
    // A profile deleted in Credential profiles is no longer picked.
    if (profileId && !profiles.some((entry) => entry.id === profileId)) profileId = null;
  }

  $effect(() => {
    void profilesKey;
    void loadProfiles();
  });

  /** The last option of the profile picker, which opens Credential profiles instead of picking. */
  const MANAGE = "manage:";
  const profileOptions = $derived([
    ...profiles.map((entry) => ({ value: entry.id, label: entry.name, meta: `Signs in as ${entry.user}`, icon: "key" as const })),
    { value: MANAGE, label: "Manage profiles…", icon: "settings" as const },
  ]);

  function pickProfile(id: string) {
    if (id === MANAGE) {
      onManageProfiles(profileId);
      return;
    }
    profileId = id;
    madeProfile = null;
  }

  /** Keeps the user and password typed here as a new profile named after the user, and signs in with it. */
  async function saveAsProfile() {
    const login = user.trim();
    if (!login || busy) return;
    const taken = (name: string) => profiles.some((entry) => entry.name.toLowerCase() === name.toLowerCase());
    let name = login;
    for (let n = 2; taken(name); n += 1) name = `${login}-${n}`;
    busy = true;
    try {
      const stored = await ipc.saveProfile({ id: "", name, user: login }, password !== "" ? password : null);
      await loadProfiles();
      profileId = stored.id;
      useProfile = true;
      madeProfile = stored.name;
      error = null;
    } catch (caught) {
      error = asCoreError(caught);
    }
    busy = false;
  }

  const CREDENTIALS = [
    { value: false, label: "Type them here" },
    { value: true, label: "From a profile" },
  ];

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
      dot: entry.color ?? "gray",
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
    const ssh = buildSsh();
    if (ssh === false) return null;
    return {
      engine,
      ssh,
      target: {
        kind: "network",
        host: host.trim(),
        port: portNumber,
        user: withProfile ? (profile?.user ?? null) : usesLogin ? user.trim() || null : null,
        password: usesLogin && !withProfile ? password : "",
        database: database.trim() || null,
        tls,
      },
      options,
    };
  }

  /** The form as a saved connection: a new one, or the one being edited. */
  function formConnection(config: ConnectionConfig): SavedConnection {
    return {
      id: savedId ?? "",
      name: name.trim() || defaultName(),
      config,
      savePassword: usesLogin && savePassword && !withProfile,
      lastUsed: saved?.lastUsed ?? null,
      profileId: withProfile ? profileId : null,
      color,
    };
  }

  /** The password typed now, to keep with the connection; blank keeps the one kept. */
  function typedPassword(): string | null {
    return usesLogin && !withProfile && password !== "" ? password : null;
  }

  /** Saves the form to the library; resolves to whether it worked. */
  async function saveToLibrary(config: ConnectionConfig): Promise<boolean> {
    try {
      const stored = await ipc.saveConnection(formConnection(config), typedPassword());
      savedId = stored.id;
      name = stored.name;
      onSaved();
      return true;
    } catch (caught) {
      error = asCoreError(caught);
      return false;
    }
  }

  async function submit() {
    if (busy) return;
    const config = buildConfig();
    if (!config) return;
    busy = true;
    unknownHost = null;
    // With Save this connection, the connection is saved only once the server has
    // answered, so a mistyped host or login is never kept; a password left blank
    // on a saved connection uses the one kept in Credential Manager.
    error = askTrust(
      await onConnect({
        config,
        profileId: withProfile ? profileId : null,
        title: name.trim() || defaultName(),
        keep: save ? { connection: formConnection(config), password: typedPassword() } : null,
      }),
      () => void submit(),
    );
    if (save && !error) onSaved();
    busy = false;
  }

  // --- The Database picker: what this login may see, read as the list opens ---

  /** The login the list is read with; it is read again when one of these changes. */
  const loginFields = $derived(
    JSON.stringify([engine, host, port, user, password, windowsLogin, tls, options, withProfile, profileId, savedId, sshOn, sshHost, sshPort, sshUser, sshWithKey, sshPassword, sshKeyPath, sshPassphrase]),
  );
  let listed: { fields: string; names: string[] | null; error: CoreError | null } | null = $state(null);
  let listing = $state(false);

  /** Connects as Test does, without a database, and keeps the names the login may see. */
  async function listDatabases() {
    if (listing || host.trim() === "" || listed?.fields === loginFields) return;
    const built = buildConfig();
    if (!built) return;
    const config: ConnectionConfig = built.target.kind === "network" ? { ...built, target: { ...built.target, database: null } } : built;
    const fields = loginFields;
    listing = true;
    try {
      listed = { fields, names: await ipc.listFormDatabases(config, savedId, withProfile ? profileId : null), error: null };
    } catch (caught) {
      const failure = asCoreError(caught);
      listed = { fields, names: null, error: failure };
      askTrust(failure, () => {
        listed = null;
        void listDatabases();
      });
    }
    listing = false;
  }

  /** Values no database can have: the rows that only say what the list is doing. */
  const NOTE = "\u0000note";
  const databaseOptions = $derived.by(() => {
    const items: ComboOption<string>[] = [{ value: "", label: "Default", meta: "The database the login opens on" }];
    const current = listed?.fields === loginFields ? listed : null;
    if (host.trim() === "") items.push({ value: NOTE, label: "Fill in the host to list the databases", disabled: true });
    else if (listing) items.push({ value: NOTE, label: "Reading the databases…", icon: "loader", disabled: true });
    else if (current?.error) items.push({ value: NOTE, label: errorTitle(current.error), meta: current.error.message, disabled: true });
    for (const name of current?.names ?? []) items.push({ value: name, label: name });
    const typed = database.trim();
    if (typed && !items.some((item) => item.value === typed)) items.push({ value: typed, label: typed });
    return items;
  });

  /** The fields a test depends on: a result shows only while they are as tested. */
  const testedFields = $derived(
    JSON.stringify([engine, host, port, database, user, password, windowsLogin, tls, account, options, withProfile, profileId, sshOn, sshHost, sshPort, sshUser, sshWithKey, sshPassword, sshKeyPath, sshPassphrase]),
  );
  let testing = $state(false);
  let tested: { fields: string; title: string; message: string } | null = $state(null);

  /** Connects and closes again, to show the login works before anything is saved or opened. */
  async function test() {
    if (busy || testing) return;
    const config = buildConfig();
    if (!config) return;
    const fields = testedFields;
    testing = true;
    error = null;
    tested = null;
    unknownHost = null;
    try {
      const reply = await ipc.testConnection(config, savedId, withProfile ? profileId : null);
      const version = `${info.title} ${reply.serverVersion ?? ""}`.trim();
      const count = reply.databases === null ? "" : ` · this login sees ${reply.databases} ${reply.databases === 1 ? "database" : "databases"}`;
      tested = { fields, title: sshOn ? "Connected through SSH" : "Connected", message: version + count };
    } catch (caught) {
      error = askTrust(asCoreError(caught), () => void test());
    }
    testing = false;
  }

  async function saveOnly() {
    const config = buildConfig();
    if (!config || busy) return;
    busy = true;
    if (await saveToLibrary(config)) onClose();
    busy = false;
  }

  /** The `--conn-*` tokens, grey (no colour) first. */
  const COLORS = ["gray", "red", "orange", "green", "teal", "blue", "purple"] as const;

  const LOGINS = [
    { value: false, label: "SQL Server login" },
    { value: true, label: "Windows account" },
  ];
</script>

<Dialog title={saved ? `Edit ${saved.name}` : "New connection"} width={600} {busy} {onClose} onsubmit={() => void submit()}>
  {#if error}<InfoBar tone="danger" title={errorTitle(error)} message={error.message} />{/if}
  {#if unknownHost}
    <InfoBar
      tone="warning"
      title={`${unknownHost.host} is not a known SSH server`}
      message={`Its key is ${unknownHost.fingerprint}. Trust it only if your server's administrator gives the same fingerprint.`}
    >
      <Button size="sm" label="Trust this key" tip="Keep it in Kemable's known hosts and go on" onclick={() => void trustHost()} />
    </InfoBar>
  {/if}
  {#if tested && tested.fields === testedFields && !error}<InfoBar tone="success" title={tested.title} message={tested.message} />{/if}

  <div class="pb-form-2">
    <Field label="Engine" as="group">
      <Combobox label="Engine" width={268} options={engineOptions} value={engine} onPick={pickEngine} />
    </Field>
    <Field label="Saved connection" as="group">
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
    </Field>
  </div>

  {#if http}
    <div class="pb-form-2">
      <Field label="Account ID">
        <Input mono bind:ref={accountInput} bind:value={account} required placeholder="32 letters and digits, from the dashboard" />
      </Field>
      <Field label="Database">
        <Input mono bind:value={database} placeholder="Optional: every database shows in the tree" />
      </Field>
    </div>
    <Field label="API token" hint="Each statement is its own request and its own transaction. Local D1 databases are SQLite files: open them with Open file.">
      <Input
        type="password"
        bind:value={password}
        placeholder={savedId && saved?.savePassword ? "Kept in Windows Credential Manager" : "A token with D1 Read (and Edit to write)"}
      />
    </Field>
  {:else}
    <Field label="Connection URL" hint="Paste a URL (Neon, Supabase, PostgreSQL, SQL Server or Redis) to fill the fields below.">
      <Input mono bind:ref={urlInput} bind:value={url} onchange={fillFromUrl} onpaste={() => setTimeout(fillFromUrl)} placeholder={info.urlHint} />
    </Field>
  {/if}

  <div class="pb-form-grid pb-form-grid--auto">
    <Field label="Name"><Input bind:value={name} placeholder={defaultName() || "My database"} /></Field>
    <Field label="Colour" as="group">
      <Swatches label="Connection colour" colors={COLORS} value={color} onChange={(next) => (color = next)} />
    </Field>
  </div>

  {#if !http}
    <div class="pb-form-grid">
      <Field label="Host"><Input bind:value={host} required placeholder={info.hostHint} /></Field>
      <Field label="Port"><Input class="pb-num" bind:value={port} placeholder={String(info.port)} inputmode="numeric" /></Field>
    </div>
    {#if engine === "redis"}
      <Field label="Database number"><Input bind:value={database} placeholder="0" inputmode="numeric" /></Field>
    {/if}
    {#if engine === "mssql"}
      <Field label="Sign in with" as="group">
        <Segmented label="Sign in with" fill options={LOGINS} value={windowsLogin} onChange={(windows) => (windowsLogin = windows)} />
      </Field>
    {/if}
    {#if profileEngine}
      <Field label="Credentials" as="group">
        <Segmented label="Credentials" fill options={CREDENTIALS} value={useProfile} onChange={(next) => (useProfile = next)} />
      </Field>
    {/if}
    {#if withProfile}
      {#if madeProfile && profile?.name === madeProfile}
        <InfoBar tone="success" title={`Saved as the profile ${madeProfile}`} message="Its password is kept in Windows Credential Manager. Any connection can use it." />
      {/if}
      <Field
        label="Profile"
        as="group"
        hint={profile
          ? `Signs in as ${profile.user}. The password is the profile's, kept in Windows Credential Manager.`
          : "A profile keeps a user and a password under a name, for any connection to use."}
      >
        <Combobox
          label="Profile"
          width={552}
          searchable={profiles.length > 6}
          searchPlaceholder="Find a profile"
          options={profileOptions}
          value={profileId ?? ""}
          placeholder="Pick a profile"
          onPick={pickProfile}
        />
      </Field>
    {:else if usesLogin}
      <div class="pb-form-2">
        <Field label="User">
          <Input bind:value={user} placeholder={engine === "postgres" ? "postgres" : engine === "redis" ? "Optional (ACL user)" : "sa"} />
        </Field>
        <Field label="Password">
          <Input type="password" bind:value={password} placeholder={savedId && saved?.savePassword ? "Kept in Windows Credential Manager" : ""} />
        </Field>
      </div>
      {#if profileEngine}
        <div class="pb-row pb-row--start">
          <Button
            variant="ghost"
            size="sm"
            icon="key"
            disabled={busy || user.trim() === ""}
            tip="Keep this user and password under a name, to use them in other connections"
            tipAt="start"
            onclick={() => void saveAsProfile()}>Save these as a profile…</Button
          >
        </div>
      {/if}
    {/if}
    <Field label="Encryption" as="group">
      <Combobox label="Encryption" width={552} options={tlsModes} value={tls} onPick={(mode) => (tls = mode)} />
    </Field>
    <Checkbox bind:checked={sshOn}>Connect through an SSH tunnel</Checkbox>
    {#if sshOn}
      <div class="pb-form-grid">
        <Field label="SSH host"><Input bind:value={sshHost} required placeholder="bastion.example.com" /></Field>
        <Field label="SSH port"><Input class="pb-num" bind:value={sshPort} placeholder="22" inputmode="numeric" /></Field>
      </div>
      <div class="pb-form-2">
        <Field label="SSH user"><Input bind:value={sshUser} required placeholder="ubuntu" /></Field>
        <Field label="SSH sign-in" as="group">
          <Segmented label="SSH sign-in" fill options={SSH_LOGINS} value={sshWithKey} onChange={(key) => (sshWithKey = key)} />
        </Field>
      </div>
      {#if sshWithKey}
        <div class="pb-form-grid pb-form-grid--auto">
          <Field label="Private key file"><Input mono bind:value={sshKeyPath} required placeholder="C:\Users\me\.ssh\id_ed25519" /></Field>
          <Field label={" "} as="group"><Button icon="file" onclick={() => void pickKey()}>Browse…</Button></Field>
        </div>
        <Field label="Key passphrase">
          <Input type="password" bind:value={sshPassphrase} placeholder={sshKept ? "Kept in Windows Credential Manager, or none" : "Empty when the key has none"} />
        </Field>
      {:else}
        <Field label="SSH password">
          <Input type="password" bind:value={sshPassword} placeholder={sshKept ? "Kept in Windows Credential Manager" : ""} />
        </Field>
      {/if}
      <p class="pb-hint">The host and port above are the database's as the SSH server sees them, often localhost. The SSH password or passphrase is kept in Windows Credential Manager.</p>
    {/if}
    {#if engine !== "redis"}
      <!-- After the login: the list is what that login may see. -->
      <Field label="Database" as="group" hint="Optional. The tree shows every database this login may see.">
        <Combobox
          label="Database"
          width={552}
          searchable
          searchPlaceholder="Find, or type a name"
          options={databaseOptions}
          value={database.trim()}
          placeholder="Default"
          onOpen={() => void listDatabases()}
          onPick={(name) => (database = name)}
          custom={(text) => ({ value: text, label: `Use “${text}”`, meta: "A database not in the list" })}
        />
      </Field>
    {/if}
  {/if}

  <div class="pb-checks">
    <Checkbox bind:checked={save}>Save this connection</Checkbox>
    {#if save && usesLogin && !withProfile}
      <Checkbox class="pb-check--indent" bind:checked={savePassword}>Keep the {http ? "token" : "password"} in Windows Credential Manager</Checkbox>
    {/if}
  </div>
  {#if Object.keys(options).length > 0}
    <p class="pb-hint">
      From the URL: {Object.entries(options)
        .map(([key, value]) => `${key}=${value}`)
        .join(", ")}
    </p>
  {/if}

  {#snippet footer()}
    {#if savedId}
      {#if confirmDelete}
        <Confirm size="md" question={`Delete ${name || "it"}?`} action="Delete" {busy} onConfirm={remove} onCancel={() => (confirmDelete = false)} />
      {:else}
        <Button variant="ghost" icon="trash" disabled={busy} tip="Delete this saved connection" tipAt="start" onclick={() => (confirmDelete = true)}>Delete…</Button>
      {/if}
    {/if}
    {#if !confirmDelete}
      <Button
        icon={testing ? "loader" : undefined}
        spin={testing}
        disabled={busy || testing || !ready}
        tip="Connect and close again, to check the login"
        tipAt="start"
        onclick={() => void test()}>{testing ? "Testing…" : "Test"}</Button
      >
    {/if}
    <Spacer />
    <Button variant="ghost" disabled={busy} onclick={onClose}>Cancel</Button>
    {#if save}<Button disabled={busy || !ready} onclick={saveOnly}>Save</Button>{/if}
    <Button variant="primary" type="submit" icon={busy ? "loader" : undefined} spin={busy} disabled={busy || !ready} tip="Connect  Enter" tipAt="end"
      >{busy ? "Connecting…" : "Connect"}</Button
    >
  {/snippet}
</Dialog>
