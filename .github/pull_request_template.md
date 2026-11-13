<!--
Open the pull request against `test`; `main` takes what has been tested there.
Title: type(scope): description, e.g. feat(core-store): pack windows column by column
Types: feat, fix, perf, refactor, test, docs, chore
Scopes: core, core-<crate>, driver-<engine>, windows, docs, repo
-->

## What and why

<!-- What this changes and why. -->

Closes #<!-- issue number, or delete this line -->

## How to test

<!-- Steps a reviewer follows to see the change work: the engine, the data, what to click, what should appear. -->

1.
2.

## Checks

- [ ] `node scripts/verify.mjs` prints `PASS` (build, `pnpm check`, `cargo fmt`, clippy, tests)
- [ ] A test covers the new or fixed behavior
- [ ] A test changed on purpose is named here, with why: <!-- or "none" -->
- [ ] What only a person can check has its steps in `docs/testing/` <!-- or "nothing to check by hand" -->
- [ ] Heavy work runs in Rust, in `core/` when it is not about one UI; no driver, SQL or formatting logic in TypeScript
- [ ] No secret, password, connection string, API key or private data in the code, tests, logs or screenshots
- [ ] No code from the commercial (premium) edition

## Speed

<!--
Startup, scrolling and query streaming must stay smooth.
If this change could slow any of them, say what you measured, how, and the numbers before and after.
Otherwise write: No effect on speed.
-->

## Docs

- [ ] A choice that shapes more than one file has an ADR in `docs/decisions/` <!-- or "no new decision" -->
- [ ] The docs that name what changed are updated (`AGENTS.md`, `core/ARCHITECTURE.md`, `docs/roadmap.md`, ...)

## Screenshots

<!-- For a UI change: before and after, in light and dark. Delete this section otherwise. -->

## Contributor License Agreement

- [ ] I have read the [Contributor License Agreement](https://github.com/SoleBuilt/Kemable/blob/main/CLA.md). If this is my first pull request, I sign it by commenting when the CLA Assistant check asks.
