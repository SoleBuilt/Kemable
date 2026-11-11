// Query pages kept across launches (docs/decisions/0013-pages-and-tabs.md):
// each connection's query tabs, their text and cursor, in the WebView's
// storage. Table pages and results are not kept; they come from the server.

import type { OpenedConnection } from "./types";

/** One query page as kept: its number ("Query 2"), its text and where the cursor was. */
export interface KeptPage {
  number: number;
  text: string;
  cursor: number;
}

export interface KeptPages {
  pages: KeptPage[];
  /** The index in `pages` of the page that was shown. */
  active: number;
  /** When they were kept, in milliseconds, to drop the oldest connections first. */
  at: number;
}

const KEY = "paboable.pages";
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

function isPage(value: unknown): value is KeptPage {
  if (typeof value !== "object" || value === null) return false;
  const page = value as Record<string, unknown>;
  return typeof page.number === "number" && typeof page.text === "string" && typeof page.cursor === "number";
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
