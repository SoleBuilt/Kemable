---
name: clean-target
description: >-
  Shrink the Rust target/ folder to what the verify commands and `tauri dev` still use. Use when
  the user asks to clean target, free disk space, or says target is too big: "dọn target",
  "dọn dẹp thư mục target", "target quá lớn", "giải phóng dung lượng". Not for a full `cargo clean`.
---

# Clean target

Run one command from the repository root, then report its last two lines to the user:

```bash
node .claude/skills/clean-target/clean-target.mjs
```

That is the whole task. Do not measure folders before or after, sample files, or re-run clippy,
test or build to confirm. The script already does the work and prints the result.

## What it does

1. Runs the cargo commands that `scripts/verify.mjs` runs (`clippy --all-targets -D warnings`,
   `test --no-run`) and `cargo build --workspace` (for `tauri dev`), with `--message-format=json`.
   Cargo then lists every artifact it uses. Units that are already fresh do not compile again.
   Units that are stale do, so the first run after a change can take a minute or two.
2. Deletes `target/release` (verify never uses it) and `target/debug/incremental` (rustc builds it
   again the next time a workspace crate changes).
3. Deletes each file in `target/debug/deps` and each folder in `target/debug/build` that the cargo
   output does not mention. Those are artifacts of old dependency versions and old settings.

It keeps `.fingerprint`, `flycheck0` and the top-level binaries. They are small.

## When it prints something else

- `skipped deps/ and build/`: a cargo command failed, usually because the code does not compile.
  Release and incremental were still removed. Tell the user; deps and build can be cleaned once the
  build passes.
- `N locked and left in place`: the app or rust-analyzer had those files open. Say so; closing the
  app and running again removes them.

Run `cargo clean` only when the user asks for it by name. It deletes everything, and the next
build starts from nothing.
