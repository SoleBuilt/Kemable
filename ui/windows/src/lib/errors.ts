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
  "filter.raw": "A SQL condition can filter a table, not a query result",
  "filter.invalid": "This filter needs a column of the result and a value",
  "file.not_text": "This file is not UTF-8 or UTF-16 text",
  "ssh.connect": "Could not reach the SSH server",
  "ssh.auth": "The SSH server refused the login",
  "ssh.host_unknown": "This SSH server is not known yet",
  "ssh.host_changed": "This SSH server's key has changed",
  "ssh.forward": "The SSH server would not reach the database",
  "update.signature": "The update is not signed by Kemable",
  "storage.unreadable": "A file Kemable keeps cannot be read; it is left as it is",
  "secret.moved": "This connection points elsewhere than where its password was kept for: enter the password again",
  "secret.vault": "Windows Credential Manager could not keep or read the password",
  "ssh.secret_moved": "This connection's SSH server changed since its SSH password was kept: edit the connection to enter it again",
  internal: "Something went wrong inside Kemable",
};

export function errorTitle(error: CoreError): string {
  return TITLES[error.code] ?? error.code;
}
