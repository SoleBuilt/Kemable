// Query pages kept across launches (docs/decisions/0013-pages-and-tabs.md):
// each connection's query tabs, their text and cursor, one file a connection
// in the data folder (docs/decisions/0019-data-folder.md). Table pages and
// results are not kept; they come from the server.

import { ipc } from "./ipc";
import { pagesWithoutConnection, readPref, ready, track, writePref } from "./store";
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
  /** When they were kept, in milliseconds. */
  at: number;
}

/** Where the pages of `.sql` files opened with no connection are kept. */
export const NO_CONNECTION = "none";

/**
 * Names a connection across launches: a saved connection by its id, a SQLite
 * file by its path, anything else by its engine and title (host:port).
 */
export function connectionKey(opened: OpenedConnection, filePath: string | null): string {
  if (opened.savedId) return `saved:${opened.savedId}`;
  if (filePath) return `file:${filePath}`;
  return `${opened.engine.id}:${opened.title}`;
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

/** What was kept, if it reads as pages. */
function asPages(kept: unknown): KeptPages | null {
  if (typeof kept !== "object" || kept === null) return null;
  const read = kept as Partial<KeptPages>;
  if (!Array.isArray(read.pages)) return null;
  const pages = read.pages.filter(isPage);
  if (pages.length === 0) return null;
  const active = typeof read.active === "number" && read.active >= 0 && read.active < pages.length ? read.active : 0;
  return { pages, active, at: typeof read.at === "number" ? read.at : 0 };
}

/** The pages kept for `conn`, or null when there are none (or what is kept is not readable). */
export async function readPages(conn: string): Promise<KeptPages | null> {
  await ready;
  return asPages(await ipc.readPages(conn).catch(() => null));
}

/** The pages of files opened with no connection, at once, so the first paint shows them. */
export function readPagesWithoutConnection(): KeptPages | null {
  return asPages(pagesWithoutConnection());
}

/** Keeps `kept` for `conn`; closing the window waits for it. A folder that cannot be written keeps nothing. */
export function writePages(conn: string, kept: KeptPages): void {
  void track(ready.then(() => ipc.writePages(conn, kept))).catch(() => {});
}

// --- Recent files, for Ctrl+K ------------------------------------------------

const MAX_RECENT = 10;

/** The `.sql` files opened or saved last, newest first. */
export function readRecentFiles(): string[] {
  const kept = readPref("recentFiles");
  return Array.isArray(kept) ? kept.filter((path): path is string => typeof path === "string").slice(0, MAX_RECENT) : [];
}

/** Puts `path` first among the recent files; returns the list. */
export function addRecentFile(path: string): string[] {
  const same = (other: string) => other.toLowerCase() === path.toLowerCase();
  const list = [path, ...readRecentFiles().filter((other) => !same(other))].slice(0, MAX_RECENT);
  writePref("recentFiles", list);
  return list;
}

/** A path's last part: the name a tab shows. */
export function fileName(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}
