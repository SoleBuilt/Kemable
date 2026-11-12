// What the app remembers between launches (docs/decisions/0019-data-folder.md).
// The Rust side keeps it in the data folder; before the UI's first line runs it
// puts the preferences, and the pages of files opened with no connection, on
// `window.__PABOABLE__`, so they read at once, as storage in the WebView did.
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
  /** An older version kept all this in the WebView's storage: move it into files, once. */
  importLocalStorage: boolean;
  folder: DataFolderInfo | null;
}

declare global {
  interface Window {
    __PABOABLE__?: Startup;
  }
}

const startup: Startup = window.__PABOABLE__ ?? { prefs: null, pagesWithout: null, importLocalStorage: false, folder: null };

export const dataFolder: DataFolderInfo | null = startup.folder;

// --- Moving an older version's storage ----------------------------------------

/** The keys an older version kept in the WebView's storage, and the preference each became. */
const OLD_KEYS: Record<string, string> = {
  "paboable.settings": "settings",
  "paboable.pageSize": "pageSize",
  "paboable.treeScope": "treeScope",
  "paboable.recentFiles": "recentFiles",
};

function readOld(key: string): unknown {
  try {
    const text = localStorage.getItem(key);
    if (text === null) return undefined;
    // The page size and the tree's scope were kept as bare text.
    try {
      return JSON.parse(text) as unknown;
    } catch {
      return text;
    }
  } catch {
    return undefined;
  }
}

function fromOldStorage(): { prefs: Record<string, unknown>; pages: Record<string, unknown> } {
  const prefs: Record<string, unknown> = {};
  for (const [old, name] of Object.entries(OLD_KEYS)) {
    const value = readOld(old);
    if (value !== undefined) prefs[name] = value;
  }
  const pages = readOld("paboable.pages");
  return { prefs, pages: typeof pages === "object" && pages !== null ? (pages as Record<string, unknown>) : {} };
}

const moved = startup.importLocalStorage ? fromOldStorage() : null;

let prefs: Record<string, unknown> = moved?.prefs ?? startup.prefs ?? {};

/** Resolves once what an older version kept is in files; a page read waits for it. */
export const ready: Promise<void> = moved
  ? ipc.importLocalStorage(moved.prefs, moved.pages).catch(() => {
      // The next launch tries again; this one goes on with what it read.
    })
  : Promise.resolve();

/** The pages of files opened with no connection, as kept. */
export function pagesWithoutConnection(): unknown {
  return moved ? moved.pages.none : startup.pagesWithout;
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
  await ready;
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
