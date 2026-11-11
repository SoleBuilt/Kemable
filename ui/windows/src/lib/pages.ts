// Query pages kept across launches (docs/decisions/0013-pages-and-tabs.md):
// each connection's query tabs, their text and cursor, in the WebView's
// storage. Table pages and results are not kept; they come from the server.

import type { LineEnding, OpenedConnection, TextEncoding } from "./types";

/** The `.sql` file a query page is (docs/decisions/0016-sql-files.md), as kept. */
export interface KeptFile {
  path: string;
  encoding: TextEncoding;
  lineEnding: LineEnding;
  /** The page's text differed from the file: it comes back as it was, unsaved. */
  dirty: boolean;
}

/** One query page as kept: its number ("Query 2"), its text and where the cursor was. */
export interface KeptPage {
  number: number;
  text: string;
  cursor: number;
  file?: KeptFile;
}

export interface KeptPages {
  pages: KeptPage[];
  /** The index in `pages` of the page that was shown. */
  active: number;
  /** When they were kept, in milliseconds, to drop the oldest connections first. */
  at: number;
}

const KEY = "paboable.pages";
/** Where the pages of `.sql` files opened with no connection are kept. */
export const NO_CONNECTION = "none";
/** The connections whose pages are kept; the least recently used go first. */
const MAX_CONNECTIONS = 40;

/**
 * Names a connection across launches: a saved connection by its id, a SQLite
 * file by its path, anything else by its engine and title (host:port).
 */
export function connectionKey(opened: OpenedConnection, filePath: string | null): string {
  if (opened.savedId) return `saved:${opened.savedId}`;
  if (filePath) return `file:${filePath}`;
  return `${opened.engine.id}:${opened.title}`;
}

function readAll(): Record<string, KeptPages> {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(KEY) ?? "{}");
    return typeof parsed === "object" && parsed !== null ? (parsed as Record<string, KeptPages>) : {};
  } catch {
    return {};
  }
}

function isFile(value: unknown): value is KeptFile {
  if (typeof value !== "object" || value === null) return false;
  const file = value as Record<string, unknown>;
  return (
    typeof file.path === "string" &&
    ["utf8", "utf8Bom", "utf16Le", "utf16Be"].includes(String(file.encoding)) &&
    (file.lineEnding === "crlf" || file.lineEnding === "lf") &&
    typeof file.dirty === "boolean"
  );
}

function isPage(value: unknown): value is KeptPage {
  if (typeof value !== "object" || value === null) return false;
  const page = value as Record<string, unknown>;
  return (
    typeof page.number === "number" &&
    typeof page.text === "string" &&
    typeof page.cursor === "number" &&
    (page.file === undefined || isFile(page.file))
  );
}

/** The pages kept for `conn`, or null when there are none (or what is kept is not readable). */
export function readPages(conn: string): KeptPages | null {
  const kept = readAll()[conn];
  if (!kept || !Array.isArray(kept.pages)) return null;
  const pages = kept.pages.filter(isPage);
  if (pages.length === 0) return null;
  const active = typeof kept.active === "number" && kept.active >= 0 && kept.active < pages.length ? kept.active : 0;
  return { pages, active, at: typeof kept.at === "number" ? kept.at : 0 };
}

/** Keeps `kept` for `conn`. Without storage, or when it is full, the pages last only until the app closes. */
export function writePages(conn: string, kept: KeptPages): void {
  const all = readAll();
  all[conn] = kept;
  const names = Object.keys(all).sort((a, b) => (all[b]?.at ?? 0) - (all[a]?.at ?? 0));
  for (const old of names.slice(MAX_CONNECTIONS)) delete all[old];
  try {
    localStorage.setItem(KEY, JSON.stringify(all));
  } catch {
    // A full or missing storage keeps nothing; the pages still work until the app closes.
  }
}

// --- Recent files, for Ctrl+K ------------------------------------------------

const RECENT_KEY = "paboable.recentFiles";
const MAX_RECENT = 10;

/** The `.sql` files opened or saved last, newest first. */
export function readRecentFiles(): string[] {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]");
    return Array.isArray(parsed) ? parsed.filter((path): path is string => typeof path === "string").slice(0, MAX_RECENT) : [];
  } catch {
    return [];
  }
}

/** Puts `path` first among the recent files; returns the list. */
export function addRecentFile(path: string): string[] {
  const same = (other: string) => other.toLowerCase() === path.toLowerCase();
  const list = [path, ...readRecentFiles().filter((other) => !same(other))].slice(0, MAX_RECENT);
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify(list));
  } catch {
    // Without storage the list lasts until the app closes.
  }
  return list;
}

/** A path's last part: the name a tab shows. */
export function fileName(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}
