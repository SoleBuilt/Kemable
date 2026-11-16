# 0028: The app updates itself from GitHub Releases, asked once at launch, with installers only CI signs

- **Status**: Accepted
- **Date**: 2026-11-14

## Context

Decided with the user on 2026-11-12 (`docs/roadmap.md`, Update in place), to build once the repository and the license were settled. They are (`docs/decisions/0023-licensing.md`), and the first installer is about to reach other people. An installer without an updater leaves everyone on it until they come back to download by hand, so the updater has to be in the first release.

What Tauri gives, read in `tauri-plugin-updater` 2.14.1 and the NSIS template of `tauri-bundler` 2.10:

- The updater reads a JSON file at a URL (`version`, `notes`, `pub_date`, and per platform a `url` and a `signature`), keeps the whole file as `raw_json`, and offers the release when its version is above the app's.
- The installer it downloads must match its minisign signature against `plugins.updater.pubkey` in `tauri.conf.json`; a mismatch installs nothing. `pnpm tauri build` writes the signature when `bundle.createUpdaterArtifacts` is on and `TAURI_SIGNING_PRIVATE_KEY` is set.
- On Windows it runs the NSIS installer with `/P` (passive: a progress window, no questions), `/UPDATE` and `/R` (start the app again), then ends the app's process. In update mode the installer does not uninstall first, so `data\` beside the app stays (`installer/hooks.nsh` also skips it when `$UpdateMode` is set).
- With `installMode: "both"` the installer runs at the highest level the user has (`MULTIUSER_EXECUTIONLEVEL Highest`) and finds the earlier install's mode in the registry. An app installed for Everyone is in Program Files, so the update asks Windows for admin rights (UAC) for an account that is an administrator.

## Decision

**Where.** `https://github.com/SoleBuilt/Kemable/releases/latest/download/latest.json`, the static file of the latest published release (`plugins.updater.endpoints`). Its one platform is `windows-x86_64`.

**When.** The UI asks once at launch (`App.svelte`), never on a timer, and only when Settings › Updates › Check for a new version when Kemable opens is on (the default). No network, or any failure, says nothing. A development build (`pnpm tauri dev`) does not ask at launch: its installer would replace the installed app. Settings › Updates › Check now asks at any time and says what it found or why it could not.

**What the check sends.** One `GET` of `latest.json` with the user agent `tauri-plugin-updater/<its version>`. The URL holds no version, no Windows build and no architecture, so GitHub learns less than the roadmap allowed (the version, Windows and the architecture): what any download tells it, the IP address. Settings says so beside the switch. The installer is then downloaded from the same release.

**The notice** (`lib/shell/UpdateDialog.svelte`): "Kemable 0.2.0 is available", the version you have, what changed (the release's notes, as plain text), and three answers:

- **Update.** It first lists what closing the app now would stop or leave unsaved: a query running, a `.sql` file's page with changes not in its file, a table tab's edits not saved, a Live tab listening. Back, or Update anyway. Then it writes the pages, the preferences and the window's place, downloads with its progress (at most every 50 ms, `src-tauri/src/updates.rs`), checks the signature and starts the installer, which ends the app and opens the new version. A failure is shown in the notice, with Try again.
- **Later** (or Esc). The launch check offers the same version again 3 days later. After the 7th Later it is not offered at launch again; Check now still shows it.
- **Skip this version.** The launch check never offers this version again; a newer one is offered.

Later and Skip are kept in `ui.json` as `updates: { skipped, later: { version, until, times } }` (`lib/updates.ts`).

**A critical fix** is a release whose `latest.json` has `"critical": true`. Its notice has no Later, no Skip and cannot be closed: Update, or Close Kemable, and the next launch asks again. The release workflow writes the field from the notes' front matter (`critical: true`, `docs/releases/README.md`). With the launch check turned off the app does not learn of it; that is the user's choice.

**Commands, not the plugin's JavaScript.** `check_update` and `install_update` (`src-tauri/src/updates.rs`) call the plugin from Rust. The UI reaches them through `lib/ipc.ts` like every other command, and `capabilities/default.json` gains nothing. A signature that does not match is `update.signature`; a network failure `driver.connect` or `driver.timeout`.

**Releasing** (`.github/workflows/release.yml`, `scripts/release.mjs`):

1. In one change: raise the version in `Cargo.toml` (`[workspace.package]`), `ui/windows/package.json` and `ui/windows/src-tauri/tauri.conf.json`, and write `docs/releases/<version>.md`.
2. Once it is on `main`: `git tag v0.2.0` and `git push origin v0.2.0`.
3. The workflow checks that the tag is the version in all three files and that the notes exist, builds the installer with `createUpdaterArtifacts`, signs it with the key in the repository's secrets, writes `latest.json`, and drafts a release with the installer, its `.sig` and `latest.json`.
4. Read the draft on GitHub and publish it, not as a pre-release: `releases/latest` skips drafts and pre-releases, so the app sees a release only once it is published.

**The key, once** (only the person who keeps it; the private key never enters the repository):

1. In `ui/windows`: `pnpm tauri signer generate -w C:\Users\<you>\.tauri\kemable-updater.key`, and give it a password when asked. It writes the private key there and the public key beside it as `kemable-updater.key.pub`.
2. GitHub › SoleBuilt/Kemable › Settings › Secrets and variables › Actions › New repository secret: `TAURI_SIGNING_PRIVATE_KEY` is the whole content of `kemable-updater.key`; `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` is its password.
3. Put the content of `kemable-updater.key.pub` in `plugins.updater.pubkey` in `ui/windows/src-tauri/tauri.conf.json`, in place of `REPLACE_WITH_THE_UPDATER_PUBLIC_KEY_SEE_DOCS_DECISIONS_0028`, and commit it.
4. Keep the key file and its password somewhere safe outside this PC as well.

Until step 3 the app checks and offers an update, and its download stops at `update.signature`: nothing unsigned is ever installed.

**Authenticode** (added 2026-11-15, `docs/preview-audit.md` C19). The updater's signature proves an installer to the app; Windows knows nothing of it, so SmartScreen says "Windows protected your PC" and "Unknown publisher" on the first install. An Authenticode signature names the publisher. It needs a code-signing certificate, which only the owner can buy. The release workflow is ready for one and skips it while there is none:

- When the secrets `WINDOWS_CERTIFICATE` (the PFX file, base64) and `WINDOWS_CERTIFICATE_PASSWORD` exist, the workflow imports the certificate into the runner's store and passes its thumbprint to the build (`bundle.windows.certificateThumbprint`, SHA-256, timestamped by DigiCert's RFC 3161 server). Tauri then runs signtool on `Kemable.exe`, the uninstaller and the installer as it makes them, and only then writes the updater's signature. Signing the installer after the build would change the bytes the `.sig` covers, and every update would stop at `update.signature`. A last step checks that the installer's signature is valid.
- Without them, the build is as before, unsigned for Windows.

Once a certificate is bought:

1. Get it as a `.pfx` with its private key and a password. In PowerShell: `[Convert]::ToBase64String([IO.File]::ReadAllBytes("C:\path\kemable-signing.pfx")) | Set-Clipboard`.
2. GitHub › SoleBuilt/Kemable › Settings › Secrets and variables › Actions: `WINDOWS_CERTIFICATE` is the clipboard's text, `WINDOWS_CERTIFICATE_PASSWORD` the PFX's password.
3. The next version tag builds a signed installer. Check it: the installer's Properties › Digital Signatures names SoleBuilt's certificate.

Since June 2023 certificate authorities keep a new code-signing key on a hardware token or in a cloud HSM, and many no longer hand out a PFX. With such a certificate the import step is replaced by `bundle.windows.signCommand` running the provider's signing tool (a cloud HSM's, or Azure Trusted Signing's), in a change of its own; the rest stays. An OV certificate still shows a SmartScreen warning until its downloads build a reputation.

## Rationale

- GitHub Releases already hosts the code and costs nothing; a static `latest.json` needs no server of our own.
- Once at launch is what the user asked: a check that runs while someone works is noise, and a launch is when an update disturbs the least.
- Keeping the key in CI alone means no installer built on a developer's PC can reach users.
- Notes in a file of the repository are reviewed with the change that raises the version, and the critical marker sits beside them, where it is decided.
- Commands of our own keep the UI's one door to Rust (`.claude/rules/ui-windows.md`) and the capabilities as narrow as they were.

## Consequences

- Every release from 0.1.0 on carries the updater, so everyone who installs it can be moved forward.
- A lost private key means installed apps can never take another update; they would need a new installer by hand. Changing the key means a release, signed with the old key, that carries the new public key.
- Until a code-signing certificate is in the secrets (Authenticode, above), Windows SmartScreen warns on the first install of the downloaded installer, and `README.md` says how to go past it; that sentence goes once the installer is signed. The updater's signature is a separate thing: it proves to the app that an installer came from CI.
- On a PC where Kemable was installed for Everyone, an account that is not an administrator cannot install the update: the installer would need admin rights it cannot get. An administrator updates it.
- The window's own close (its button, Alt+F4, the taskbar) reads the same list and asks before it stops a run or a Live tab or loses table edits (`lib/shell/CloseDialog.svelte`, added 2026-11-15); a `.sql` page's changes are left out there, since closing keeps them as unsaved.
- The manual checks are in `docs/testing/phase-3.md`, section 11. A real update can only be tried once a published release with a signed installer and a higher version exists.

## Alternatives considered

- **The plugin's JavaScript API** (`@tauri-apps/plugin-updater`): the same plugin, but it needs the updater's permissions in the capabilities and an npm package, and puts the download outside `ipc.ts`.
- **`tauri-apps/tauri-action`** to build and write `latest.json`: it takes the notes from the workflow, not from the repository, and has no place for the critical marker. A short script does what the app needs.
- **A check on a timer, or every time the window comes back**: rejected by the user on 2026-11-12.
- **The installer as a `.zip`** (the updater's v1 format): the v2 format downloads the `.exe` itself, so the plugin is built without its zip feature.
