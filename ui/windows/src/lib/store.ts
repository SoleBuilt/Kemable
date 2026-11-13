// What the app remembers between launches (docs/decisions/0019-data-folder.md).
// The Rust side keeps it in the data folder; before the UI's first line runs it
// puts the preferences, and the pages of files opened with no connection, on
// `window.__KEMABLE__`, so they read at once, as storage in the WebView did.
// A change is held here and written a moment later, off the UI thread; closing
// the window waits for what is not written yet (`flushStore`).
import { ipc } from "./ipc";

/** Where the data folder is: beside the app, as installed, or in the user's profile. */
export interface DataFolderInfo {
  path: string;
  besideApp: boolean;
}

interface Startup {
  prefs: Record<string, unknown> | null;
  pagesWithout: unknown;
  folder: DataFolderInfo | null;
}

declare global {
  interface Window {
    __KEMABLE__?: Startup;
  }
}

const startup: Startup = window.__KEMABLE__ ?? { prefs: null, pagesWithout: null, folder: null };

export const dataFolder: DataFolderInfo | null = startup.folder;

let prefs: Record<string, unknown> = startup.prefs ?? {};

/** The pages of files opened with no connection, as kept. */
export function pagesWithoutConnection(): unknown {
  return startup.pagesWithout;
}

// --- Preferences ----------------------------------------------------------------

/** A preference as kept, or undefined; the caller checks its shape. */
export function readPref(name: string): unknown {
  return prefs[name];
}

let prefsTimer: ReturnType<typeof setTimeout> | undefined;

/** Keeps a preference; it reaches the disk a moment later. */
export function writePref(name: string, value: unknown): void {
  prefs = { ...prefs, [name]: value };
  clearTimeout(prefsTimer);
  prefsTimer = setTimeout(() => void flushPrefs(), 300);
}

async function flushPrefs(): Promise<void> {
  if (prefsTimer === undefined) return;
  clearTimeout(prefsTimer);
  prefsTimer = undefined;
  await ipc.writePrefs(prefs).catch(() => {
    // A folder that cannot be written keeps the preferences until the app closes.
  });
}

// --- Writes in flight -------------------------------------------------------------

const inFlight = new Set<Promise<unknown>>();

/** Tracks a write so closing the window waits for it. */
export function track<T>(write: Promise<T>): Promise<T> {
  inFlight.add(write);
  void write.finally(() => inFlight.delete(write)).catch(() => {});
  return write;
}

/** Writes everything not on disk yet; the window closes after it. */
export async function flushStore(): Promise<void> {
  await flushPrefs();
  await Promise.allSettled([...inFlight]);
}
