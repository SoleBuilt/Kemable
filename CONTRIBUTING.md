# Contributing to Kemable

Thank you for helping. Kemable is a fast, light database client: one Rust core
does the work, and each operating system has its own UI. Today there is the
Windows app (Tauri 2, Svelte 5, TypeScript).

## Before you start

- For anything larger than a small fix, [open an issue](https://github.com/SoleBuilt/Kemable/issues)
  first, so we can agree on the approach before you write it.
- Read [AGENTS.md](AGENTS.md): the rules every change follows. Each folder adds
  its own: [core/AGENTS.md](core/AGENTS.md) and
  [ui/windows/AGENTS.md](ui/windows/AGENTS.md).
- Setup and running the app: [README.md](README.md).

## Branches and pull requests

- Branch off `test`: `feat/<what>`, `fix/<what>`, `docs/<what>`.
- Open the pull request against `test`. `main` takes what has been tested there.
- One logical change per pull request, described with the template the form
  opens with. Title: `type(scope): description`, for example
  `fix(windows): the tree reads again after a schema change`. The types and
  scopes are in [AGENTS.md](AGENTS.md), Commits.
- Before you ask for a review, `node scripts/verify.mjs` must print `PASS`. It
  builds, lints, runs the tests and checks the dependencies' licenses; it needs
  `cargo install --locked cargo-deny; cargo install --locked --features cli cargo-about` once.

## Dependencies

A new dependency must have a license that allows both the AGPL and the
commercial build: the list is in [deny.toml](deny.toml). After a change to
`Cargo.lock` or `ui/windows/pnpm-lock.yaml`, run `node scripts/licenses.mjs`
and commit the `ui/windows/THIRD-PARTY-NOTICES.md` it writes.

## License of your contribution

Kemable is licensed under the [AGPL-3.0-only](LICENSE) and is also sold under a
commercial license ([LICENSING.md](LICENSING.md)). On your first pull request,
you sign the [Contributor License Agreement](CLA.md) by posting the comment the
CLA Assistant check asks for. You keep your copyright.

## Reporting bugs and vulnerabilities

- Bugs and ideas: [open an issue](https://github.com/SoleBuilt/Kemable/issues/new/choose).
- Vulnerabilities: privately, as [SECURITY.md](SECURITY.md) says. Never in a
  public issue.

Everyone here follows the [Code of Conduct](CODE_OF_CONDUCT.md).
