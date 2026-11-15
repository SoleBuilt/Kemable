// When the launch check offers an update, and what Later and Skip this version
// leave in ui.json (docs/decisions/0028-updates.md). Check now in Settings
// always shows what it finds; only the launch check is held back by these.
import { readPref, writePref } from "./store";
import type { UpdateInfo } from "./types";

/** Later asks again after this long. */
export const LATER_DAYS = 3;
/** A version put off this many times is not offered at launch again. */
export const LATER_TIMES = 7;

const DAY_MS = 24 * 60 * 60 * 1000;

interface UpdatePrefs {
  /** The version Skip this version was given; never offered at launch again. */
  skipped: string | null;
  /** The version Later was given, when it may be offered again, and how many times it was put off. */
  later: { version: string; until: number; times: number } | null;
}

function readUpdatePrefs(): UpdatePrefs {
  const kept = readPref("updates");
  const saved = (typeof kept === "object" && kept !== null ? kept : {}) as Partial<UpdatePrefs>;
  const later = saved.later;
  return {
    skipped: typeof saved.skipped === "string" ? saved.skipped : null,
    later:
      typeof later === "object" &&
      later !== null &&
      typeof later.version === "string" &&
      typeof later.until === "number" &&
      typeof later.times === "number"
        ? later
        : null,
  };
}

/** Whether the launch check shows `update` now. A critical fix is always shown. */
export function offerAtLaunch(update: UpdateInfo, now = Date.now()): boolean {
  if (update.critical) return true;
  const prefs = readUpdatePrefs();
  if (prefs.skipped === update.version) return false;
  const later = prefs.later;
  if (later?.version === update.version) return later.times < LATER_TIMES && now >= later.until;
  return true;
}

/** Later: the launch check offers this version again in three days, at most seven times. */
export function putOff(version: string, now = Date.now()): void {
  const prefs = readUpdatePrefs();
  const times = prefs.later?.version === version ? prefs.later.times + 1 : 1;
  writePref("updates", { ...prefs, later: { version, until: now + LATER_DAYS * DAY_MS, times } });
}

/** Skip this version: the launch check never offers it again; a newer one is offered. */
export function skip(version: string): void {
  writePref("updates", { ...readUpdatePrefs(), skipped: version });
}
