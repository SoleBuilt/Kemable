---
name: fix-issue
description: >-
  Take a Paboable problem from report to a verified fix: read the report, find the real cause,
  fix it with every case the cause reaches, add the test that would have caught it, and verify.
  Use for bugs, wrong behavior and small features, whether phrased in English or Vietnamese
  ("sửa lỗi ...", "tại sao ... không chạy", "bị chậm khi ...", "cuộn bị giật").
  Not for a new engine (add-driver).
---

# Fix an issue

One run ends with the cause fixed, a test that fails before the fix and passes after, and `node scripts/verify.mjs` at `PASS`. Talk to the user in their language; everything written in the repo is English.

## 1. Understand

- Write down, for yourself: what happens, what should happen, the smallest way to make it happen, the engine and the data involved.
- A screenshot or recording: look at every frame around the action.
- Slowness is a bug. Ask for, or make, the query and the data size that shows it.

## 2. Find the cause

- Trace the path the report takes through the code: UI event → `ipc.ts` → command → core → driver, and back through events or windows.
- Find the mechanism, not the symptom. Then find every other input, engine and screen the same cause reaches: those are fixed in the same run.
- When the fix rests on how a library or the engine behaves, check it: read the library's source at the version in `Cargo.lock` or `pnpm-lock.yaml`, or run a small probe. An unchecked belief is a guess.

## 3. Fix

- Fix it in the layer that owns it (`AGENTS.md`, Principles): a wrong value is fixed in the driver or `format`, never patched over in the UI.
- Write the test first when you can, and watch it fail.
- No quick patch that hides the cause: no retry loop around a race, no sleep, no swallowed error.
- Read the `.claude/rules/*.md` whose `paths` match each file you edit.

## 4. Verify

- `node scripts/verify.mjs` (the `verify` skill) until `PASS`.
- For anything the user sees, `pnpm tauri dev` and do what the report did. A speed fix is measured before and after, with the numbers in the report.

## 5. Report

In the user's language: the cause in one or two sentences, what changed and where (as links), the test that pins it, the verify verdict, and what was tried by hand. Commit only when the user asks (`AGENTS.md`, Commits).
