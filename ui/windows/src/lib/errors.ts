import type { CoreError } from "./types";

// The core sends codes, never sentences (docs/decisions/0001-shared-core-and-ui-per-os.md).
// Each code gets its words here; the engine's own message follows as detail.
const TITLES: Record<string, string> = {
  "config.invalid": "The connection settings are not valid",
  "driver.connect": "Could not connect",
  "driver.auth": "Access denied",
  "driver.tls": "The secure connection failed",
  "driver.timeout": "The server took too long",
  "driver.cancelled": "Stopped",
  "driver.query": "The query failed",
  "driver.io": "A file or network error stopped the call",
  "driver.lost": "The connection was lost",
  "driver.unsupported": "Not supported by this database",
  not_found: "That item is no longer open",
  "edit.row_gone": "A row being saved is no longer there; nothing was saved",
  internal: "Something went wrong inside Paboable",
};

export function errorTitle(error: CoreError): string {
  return TITLES[error.code] ?? error.code;
}
