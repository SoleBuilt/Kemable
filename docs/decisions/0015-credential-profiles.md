# 0015: Credential profiles: a user and a password under a name, shared by saved connections

- **Status**: Accepted
- **Date**: 2026-11-11
- **Follows**: the screens "Dialog · credential profiles" and "New connection · credentials from a profile" in the Kemable Prototype, approved 2026-11-11.

## Context

Many saved connections sign in with the same login: a read-only user on every replica, a DBA account on every server. Each connection kept its own copy of the password, so changing that password meant editing every connection. The user asked for credential profiles: a login kept once and used by many connections.

## Decision

- **A profile is a name, a user and a password.** `kemable_storage::profiles` keeps the name and the user in `profiles.json` beside `connections.json`, written the same way. The password goes to the `SecretVault` under `profile/<id>/password` (Windows Credential Manager, target `profile/<id>/password.Kemable`). Names are required and unique ignoring case, since the connect form picks a profile by name.
- **A saved connection may name a profile** (`SavedConnection::profile_id`). It then keeps no secret of its own: saving it removes its kept password and turns off Keep the password. The profile's user is copied into its config, so the list and the form still show who it signs in as.
- **The core signs in.** `ConnectionLibrary::resolve` fills the user and password from the profile, so a changed profile reaches every connection that uses it the next time it connects. `connect` and `test_connection` take a `profile_id` for a connection not saved.
- **Deleting a profile** (`ConnectionLibrary::delete_profile`) removes its password and unlinks the connections that used it. They keep its user and ask for a password from then on.
- **Only SQL logins:** PostgreSQL, and SQL Server signing in with a login. Redis, D1 and SQLite do not offer profiles.
- **The UI.** The connect form has Credentials: Type them here, or From a profile, with a profile picker whose last item is Manage profiles…, and Save these as a profile… under a typed user and password. Credential profiles… in the command palette opens the dialog: the profiles on the left, the one picked on the right, with the connections it signs in.

## Consequences

- One password change in a profile reaches every connection that uses it.
- A connection on a profile cannot also keep a password of its own; switching it back to Type them here asks for one.
- Duplicate copies the name and user but not the password: the UI never reads a kept password back.
- Only one dialog answers Esc: the newest (`ui/windows/src/lib/ui/Dialog.svelte`), since Credential profiles opens over the connect form.

## Alternatives considered

- **Keep the profile inside `connections.json`.** One file to write, but a damaged connection list would also lose the profiles, and the two change at different times.
- **Copy the profile into the connection when picked.** Simpler to resolve, but then a password change would not reach the connections, which is the point of a profile.
