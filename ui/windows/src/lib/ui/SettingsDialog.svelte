<script lang="ts" module>
  export type ThemeChoice = "system" | "light" | "dark";

  /** What Settings keeps, remembered in the WebView's storage across launches. */
  export interface Settings {
    theme: ThemeChoice;
    compactRows: boolean;
    /** The row cap a run stops at; null keeps every row. */
    rowCap: number | null;
    /** After CREATE DATABASE succeeds, the rest of the script runs in it. */
    followCreatedDatabase: boolean;
  }

  /** The row caps on offer; null keeps every row. */
  export const ROW_CAPS: (number | null)[] = [1_000, 10_000, 100_000, 500_000, 1_000_000, null];

  const KEY = "paboable.settings";
  const DEFAULTS: Settings = { theme: "system", compactRows: false, rowCap: 1_000, followCreatedDatabase: true };

  export function readSettings(): Settings {
    try {
      const saved = JSON.parse(localStorage.getItem(KEY) ?? "{}") as Partial<Settings>;
      return {
        theme: saved.theme === "light" || saved.theme === "dark" ? saved.theme : "system",
        compactRows: saved.compactRows === true,
        rowCap: saved.rowCap === null || ROW_CAPS.includes(saved.rowCap ?? -1) ? (saved.rowCap ?? null) : DEFAULTS.rowCap,
        followCreatedDatabase: saved.followCreatedDatabase !== false,
      };
    } catch {
      return { ...DEFAULTS };
    }
  }

  export function writeSettings(settings: Settings): void {
    try {
      localStorage.setItem(KEY, JSON.stringify(settings));
    } catch {
      // Without storage the settings still apply until the app closes.
    }
  }

  /** Applies the theme: data-theme on <html>, or none to follow the system. */
  export function applyTheme(theme: ThemeChoice): void {
    if (theme === "system") delete document.documentElement.dataset.theme;
    else document.documentElement.dataset.theme = theme;
  }
</script>

<script lang="ts">
  import Badge from "./Badge.svelte";
  import Button from "./Button.svelte";
  import Checkbox from "./Checkbox.svelte";
  import Combobox from "./Combobox.svelte";
  import Dialog from "./Dialog.svelte";
  import Segmented from "./Segmented.svelte";
  import Setting from "./Setting.svelte";
  import Spacer from "./Spacer.svelte";

  interface Props {
    settings: Settings;
    onSave: (settings: Settings) => void;
    onClose: () => void;
  }

  let { settings, onSave, onClose }: Props = $props();

  // svelte-ignore state_referenced_locally
  let draft: Settings = $state({ ...settings });
  const numberFormat = new Intl.NumberFormat();

  // The theme shows at once, so the choice can be judged; Cancel puts the old one back.
  $effect(() => applyTheme(draft.theme));

  function cancel() {
    applyTheme(settings.theme);
    onClose();
  }

  const THEMES: { value: ThemeChoice; label: string }[] = [
    { value: "system", label: "System" },
    { value: "light", label: "Light" },
    { value: "dark", label: "Dark" },
  ];
  const STYLES = [
    { value: "mono", label: "Black and white" },
    { value: "orange", label: "Orange", disabled: true },
  ];
  const ROWS = [
    { value: false, label: "Default · 28 px" },
    { value: true, label: "Compact · 24 px" },
  ];
  const CAPS = ROW_CAPS.map((cap) => ({ value: cap, label: cap === null ? "No limit" : `${numberFormat.format(cap)} rows` }));
</script>

<Dialog title="Settings" width={560} onClose={cancel} onsubmit={() => onSave({ ...draft })} bodyStyle="gap: 16px">
  <h3 class="pb-section-title">Appearance</h3>
  <Setting title="Theme" hint="System follows Windows light or dark.">
    <Segmented label="Theme" options={THEMES} value={draft.theme} onChange={(theme) => (draft.theme = theme)} />
  </Setting>
  <Setting title="Style" hint="The orange style of ERP Tinh gọn comes later.">
    <Segmented label="Style" options={STYLES} value="mono" onChange={() => {}} />
  </Setting>
  <Setting title="Mica background" hint="The Windows 11 material behind the toolbar and sidebar.">
    <Badge>Later</Badge>
  </Setting>

  <h3 class="pb-section-title">Grid</h3>
  <Setting title="Row height" hint="Compact fits more rows on the screen.">
    <Segmented label="Row height" options={ROWS} value={draft.compactRows} onChange={(compact) => (draft.compactRows = compact)} />
  </Setting>
  <Setting title="Row limit" hint="The most rows a result keeps. Ctrl+Alt+Enter runs with no limit.">
    <Combobox label="Row limit" width={168} placement="up-end" options={CAPS} value={draft.rowCap} onPick={(cap) => (draft.rowCap = cap)} />
  </Setting>

  <h3 class="pb-section-title">Running</h3>
  <Checkbox top bind:checked={draft.followCreatedDatabase}>
    <span>After <span class="pb-mono">CREATE DATABASE</span>, run the rest of the script in the new database</span>
  </Checkbox>

  {#snippet footer()}
    <Spacer />
    <Button variant="ghost" onclick={cancel}>Cancel</Button>
    <Button variant="primary" type="submit">Save</Button>
  {/snippet}
</Dialog>
