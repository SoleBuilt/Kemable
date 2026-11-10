<script lang="ts">
  import { asCoreError, ipc } from "../ipc";
  import type { DatabaseInfo } from "../types";
  import Combobox from "../ui/Combobox.svelte";

  interface Props {
    conn: number;
    /** The database the editor runs in. */
    database: string | null;
    onPick: (database: string) => void;
    /** A list that could not be read, for the status bar. */
    onError: (message: string) => void;
  }

  let { conn, database, onPick, onError }: Props = $props();

  let databases: DatabaseInfo[] = $state.raw([]);

  /** Reads the databases this login may see; the list opens on what was read last. */
  function load() {
    ipc.listDatabases(conn).then(
      (list) => (databases = list),
      (caught: unknown) => onError(`Databases not read: ${asCoreError(caught).message}`),
    );
  }

  $effect(() => {
    void conn;
    load();
  });

  const options = $derived.by(() => {
    const list = databases.map((info) => ({ value: info.name, label: info.name, icon: "database" as const }));
    // The current database shows even before the list arrives.
    if (database !== null && !list.some((option) => option.value === database)) list.unshift({ value: database, label: database, icon: "database" });
    return list;
  });
</script>

<Combobox
  label="Database"
  width={160}
  listWidth={240}
  searchable
  searchPlaceholder="Find a database"
  tip="Switch database"
  {options}
  value={database ?? ""}
  placeholder="No database"
  onOpen={load}
  onPick={(name) => onPick(name)}
/>
