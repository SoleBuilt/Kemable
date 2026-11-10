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
  import Combobox from "./Combobox.svelte";
  import Icon from "./Icon.svelte";

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
</script>

<svelte:window
  onkeydown={(event) => {
    if (event.key === "Escape") cancel();
  }}
/>

<div class="pb-overlay pb-overlay--fixed" role="presentation">
  <form
    class="pb-dialog"
    style="width: 560px"
    aria-labelledby="settings-title"
    onsubmit={(event) => {
      event.preventDefault();
      onSave({ ...draft });
    }}
  >
    <div class="pb-dialog-head">
      <h2 class="pb-dialog-title" id="settings-title">Settings</h2>
      <button type="button" class="pb-btn pb-btn--ghost pb-btn--sm pb-btn--icon" aria-label="Close" data-tip-end="" data-tip="Close  Esc" onclick={cancel}
        ><Icon name="x" /></button
      >
    </div>
    <div class="pb-dialog-body" style="gap: 16px">
      <h3 class="pb-section-title">Appearance</h3>
      <div class="pb-setting">
        <div class="pb-setting-text"><span>Theme</span><span class="pb-hint">System follows Windows light or dark.</span></div>
        <span class="pb-seg" role="radiogroup" aria-label="Theme">
          {#each THEMES as theme (theme.value)}
            <button type="button" role="radio" aria-checked={draft.theme === theme.value} onclick={() => (draft.theme = theme.value)}>{theme.label}</button>
          {/each}
        </span>
      </div>
      <div class="pb-setting">
        <div class="pb-setting-text"><span>Style</span><span class="pb-hint">The orange style of ERP Tinh gọn comes later.</span></div>
        <span class="pb-seg" role="radiogroup" aria-label="Style">
          <button type="button" role="radio" aria-checked="true">Black and white</button>
          <button type="button" role="radio" aria-checked="false" disabled>Orange</button>
        </span>
      </div>
      <div class="pb-setting">
        <div class="pb-setting-text"><span>Mica background</span><span class="pb-hint">The Windows 11 material behind the toolbar and sidebar.</span></div>
        <span class="pb-badge">Later</span>
      </div>

      <h3 class="pb-section-title">Grid</h3>
      <div class="pb-setting">
        <div class="pb-setting-text"><span>Row height</span><span class="pb-hint">Compact fits more rows on the screen.</span></div>
        <span class="pb-seg" role="radiogroup" aria-label="Row height">
          <button type="button" role="radio" aria-checked={!draft.compactRows} onclick={() => (draft.compactRows = false)}>Default · 28 px</button>
          <button type="button" role="radio" aria-checked={draft.compactRows} onclick={() => (draft.compactRows = true)}>Compact · 24 px</button>
        </span>
      </div>
      <div class="pb-setting">
        <div class="pb-setting-text"><span>Row limit</span><span class="pb-hint">The most rows a result keeps. Ctrl+Alt+Enter runs with no limit.</span></div>
        <Combobox
          label="Row limit"
          width={168}
          placement="up-end"
          options={ROW_CAPS.map((cap) => ({ value: cap, label: cap === null ? "No limit" : `${numberFormat.format(cap)} rows` }))}
          value={draft.rowCap}
          onPick={(cap) => (draft.rowCap = cap)}
        />
      </div>

      <h3 class="pb-section-title">Running</h3>
      <label class="pb-check" style="align-items: flex-start">
        <input type="checkbox" bind:checked={draft.followCreatedDatabase} style="margin-top: 2px" />
        <span>After <span class="pb-mono">CREATE DATABASE</span>, run the rest of the script in the new database</span>
      </label>
    </div>
    <div class="pb-dialog-foot">
      <span class="pb-spacer"></span>
      <button type="button" class="pb-btn pb-btn--ghost" onclick={cancel}>Cancel</button>
      <button type="submit" class="pb-btn pb-btn--primary">Save</button>
    </div>
  </form>
</div>
