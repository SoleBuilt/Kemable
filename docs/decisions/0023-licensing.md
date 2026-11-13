# 0023: AGPL-3.0-only and a commercial license; permissive dependencies only, their notices shipped with the app

- **Status**: Accepted
- **Date**: 2026-11-13
- **Amends**: `docs/decisions/0019-data-folder.md` (the terms are no longer asked for now). Decided with the user on 2026-11-13.

## Context

Kemable is open source under the AGPL and is also to be sold under a commercial license with premium features; contributors sign a CLA (`CLA.md`) so their work can go into both. Looking at other open-source projects with a paid edition showed what was missing. Measured on 2026-11-13 with `cargo metadata` and `pnpm licenses list --prod`: about 520 Rust crates, nearly all MIT, Apache-2.0 or BSD, five MPL-2.0 (CSS parsing that Tauri pulls in), none GPL; 21 npm packages, MIT or Apache-2.0, and the Geist fonts under OFL-1.1. The icons' path data is Lucide's (ISC, MIT for the Feather ones). Nothing listed which licenses ship inside the installer, nothing stopped a GPL dependency from coming in, and the terms dialog pointed at another company's terms as stand-ins.

## Decision

- **AGPL-3.0-only**, not "or later": a future version of the license changes nothing until the owner chooses it. Declared in `Cargo.toml` (`[workspace.package] license`), `ui/windows/package.json`, `tauri.conf.json` (`bundle.license`, `bundle.copyright`, `bundle.publisher` SoleBuilt), and `README.md`. `LICENSING.md` says what is under which license and where the commercial license is asked for; `TRADEMARKS.md` keeps the names and the icon out of the AGPL (its section 7(e)).
- **Dependencies are permissive only.** `deny.toml` lists the licenses allowed for crates; `scripts/licenses.mjs` holds the same list for npm, with OFL-1.1 for fonts. GPL, LGPL and AGPL are not on it: they could not ship in the commercial build. MPL-2.0 is, since it binds only its own files.
- **The notices ship with the app.** `scripts/licenses.mjs` writes `ui/windows/THIRD-PARTY-NOTICES.md` from cargo-about (`about.toml`, `about.hbs`) for the crates, `pnpm licenses list` for the npm packages, and the license files kept beside vendored code (`LUCIDE-LICENSE`). The installer puts it beside the app (`bundle.resources`); Settings › About opens it in Notepad (`open_about`).
- **Checked on every change.** `node scripts/verify.mjs` runs `cargo deny check licenses bans sources` and `node scripts/licenses.mjs --check`, which fails on a license not allowed or a notices file out of date. `.github/workflows/licenses.yml` runs the same on every pull request and push to `test` and `main`, so a ruleset can require it before a merge.
- **The source is one click away.** Settings › About shows the version and the license, and opens the source on GitHub, the license and the notices (AGPL section 6: whoever has the app can get its source).
- **The terms dialog is hidden** (commented out in `App.svelte`), its code kept, until Kemable has terms and a privacy policy of its own.

## Rationale

The commercial license only works if every line in the build can be offered under it: Kemable's own code by copyright and the CLA, dependencies by being permissive. Checking that on each change costs a few seconds; finding a GPL crate after a sale costs the sale. MIT, Apache, BSD and OFL all ask for their notice to travel with the binary, so the notices are generated, not written by hand.

## Consequences

- Every contributor installs `cargo-deny` and `cargo-about` once. A version of cargo-about other than the one `scripts/licenses.mjs` names is refused, since it could write the same lock file differently.
- After a change to `Cargo.lock` or `pnpm-lock.yaml`, the notices are written again and committed.
- A dependency under a license not on the list needs a new decision, not an exception in the file.
- The terms are not asked for. Before Kemable is offered to others, it needs its own terms and privacy policy, and the dialog back.
- Security advisories (`cargo deny check advisories`) are not checked yet: they need the network.

## Alternatives considered

- **AGPL-3.0-or-later**: the Free Software Foundation could publish terms a buyer prefers to the commercial license.
- **Allowing LGPL and AGPL dependencies**: fine for an open-source-only app, not for one with a closed commercial build.
- **Writing the notices by hand**: about 540 packages, out of date on the next update.
