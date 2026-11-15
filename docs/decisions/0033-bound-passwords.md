# 0033: A kept password goes only where it was kept for; SSH host keys are each Windows user's own

- **Status**: Accepted
- **Date**: 2026-11-15
- **Amends**: `docs/decisions/0019-data-folder.md` (what the shared data folder holds) and `docs/decisions/0024-ssh-tunnels.md` (where Kemable's `known_hosts` is). Decided with the user on 2026-11-15 (`docs/preview-audit.md`, B15).

## Context

Installed for Everyone, Kemable keeps its data in `data\` beside the app, and every Windows user of the PC may change it (`installer/hooks.nsh`, ADR 0019). Passwords are each user's own, in Windows Credential Manager, but they were found by the connection's id alone (`connection/<id>/password`). So user A could change the host of a connection user B saved, and B's next connect sent B's password to A's server. The same held for an SSH password, and for an API token sent to a Firebase endpoint. Kemable's `known_hosts` was in `data\` too, so A could plant an SSH host key that B's tunnels then trusted.

## Decision

- **A kept secret is bound to where it is sent.** Beside the password, Credential Manager keeps what it was kept for (`connection/<id>/password-for`): the engine, the host (any case), the port, the user, and the SSH server the connection passes through; for an HTTP engine, its endpoint. An SSH password or key passphrase is bound the same way to the SSH server, its port and its user (`connection/<id>/ssh-for`). Credential Manager is never shared between Windows users, so another user cannot move the binding (`core/crates/storage/src/lib.rs`, `secret_target`, `ssh_target`).
- **When the saved connection no longer points there, the kept secret is never sent.** `resolve` leaves it out and says so (`Moved`). Opening the connection then asks for the password ("The server changed since its password was kept"), and the password typed is kept for where the connection points now (`keep_password`). A moved SSH secret is asked for in the connect form (Edit connection), since a tunnel cannot ask while it connects. The form never lends a moved secret either: Test, Databases and Connect with a blank field say the same.
- **The user's own edit moves the binding with it.** Saving the connect form with the password left blank keeps the kept password and binds it to the new host, port or user, but only while it was still kept for the connection as it was before the edit. An edit saved over a connection someone else moved leaves the password where it was.
- **Passwords kept before this change** belong to the connection as it is saved when it is first opened, and are bound to that once.
- **`known_hosts` is each Windows user's own**: `%APPDATA%\<identifier>\known_hosts` (`DataFolder::own`), never `data\`. A file in `data\` from an earlier version is not carried over, since another user may have written it: each SSH server is trusted again once, with its fingerprint shown.

## Rationale

- The shared data folder stays shared, as the user chose in ADR 0019: the saved connections are the same for everyone. Only what decides where a secret may go moves to the place each user alone can write.
- The SSH server is part of where a database password goes: through someone else's SSH server, a login without TLS is read on the way.
- Asking once after a real change costs little; sending a password to a server its owner never chose cannot be undone.

## Consequences

- A connection someone else moved, then put back, gets its kept password again: the binding never changed.
- Changing the TLS mode or the options does not ask again. A downgrade of TLS can expose a login on the network; it is left out of the binding so that the common change (turning TLS on) does not ask.
- **Not covered: a credential profile's password** (ADR 0015) is for many connections by design and is not bound to one host, so a connection someone else points at another host while it names a profile still signs in with the profile. Open until decided: bind a profile's password to the hosts it has been used with, or ask once per new host.
- Two more entries per connection in Credential Manager (`…/password-for.Kemable`, `…/ssh-for.Kemable`); deleting the connection deletes them.

## Alternatives considered

- **Keep the binding in `connections.json`**: any user who can change the host can change the binding too.
- **A data folder per Windows user for everything**: the saved connections would no longer be shared, which ADR 0019 chose.
- **Ask before every connect when the folder is shared**: safe, but the kept password would be worth nothing.
