# 0024: An SSH tunnel is a local port the app opens; drivers only dial it

- **Status**: Accepted; amended by `docs/decisions/0033-bound-passwords.md` (Kemable's `known_hosts` is each Windows user's own, and a kept SSH secret is bound to its SSH server)
- **Date**: 2026-11-14
- **Follows**: `docs/decisions/0004-engines.md` (how each engine is reached), `docs/decisions/0015-credential-profiles.md` (secrets in Windows Credential Manager), the roadmap's Phase 3 item "SSH tunnels".

## Context

Many databases are not reachable from a laptop: they listen on a private network, and the way in is an SSH server on its edge (a bastion). Desktop database clients reach them as `ssh -L` does: a local port whose every link is forwarded through the SSH server to the database's host and port, as that server sees them (often `localhost`).

Three things shape it in Kemable:

- Three drivers open TCP links (PostgreSQL, SQL Server, Redis), each its own way, and some open several: PostgreSQL has two sessions per database and one per Live tab; a cancel request opens one more.
- TLS must still name the real server: Neon and Supabase need the host name (SNI), and Verify full checks the certificate against it.
- An SSH server is trusted by its key. A client that accepts any key hands the login to whoever sits in the middle.

## Decision

### A tunnel is a local port, and a driver only dials it

- `ConnectionConfig::ssh` (`SshTunnel`: host, port, user, and a password or a private key file with its passphrase) says to go through SSH. It is saved with the connection; its secret is not.
- The app opens the tunnel (`kemable-tunnel`, on `russh` with ring, as rustls uses) before the driver connects: it checks the server's key, signs in, and opens one channel to the database's host and port at once, so a database the SSH server cannot reach is said then (`ssh.forward`) and not as a link the driver sees close. Then it listens on `127.0.0.1` and a port the system picks; each link there gets its own SSH channel.
- `ConnectionConfig::dial`, set by the app and never kept or sent to the UI, is that local address. PostgreSQL dials it through `hostaddr` and SQL Server and Redis through their own TCP connect; the host stays the server's name for TLS, SNI and pooler detection. A driver never opens a tunnel and knows nothing of SSH.
- The tunnel lives with the open server (`OpenServer`): every session, Live tab, export and cancel request of that connection goes through it, and closing the connection closes it last. Test and the connect form's database list open one for their one call.
- An SSH session that ends (sleep, a dropped VPN, the server's restart) ends the tunnel. The next call that fails on a session finds it closed and opens it again through the same server (`Tunnel::reopen`) on a new local port; the open server points its config there and drops every session, which dialed the old port, so the next call opens new ones (amended 2026-11-15, `docs/preview-audit.md` B10).
- SQL Server's named instances need the SQL Server Browser over UDP, which a tunnel does not carry: through SSH the port must be given.

### The server's key is checked, and an unknown one is asked about

- The key is checked against the user's own `~/.ssh/known_hosts` (read only, so hosts trusted with `ssh` count) and against Kemable's `known_hosts` in the Windows user's own `%APPDATA%\<identifier>` (in the data folder until ADR 0033).
- Unknown: the connection fails with `ssh.host_unknown` and the key's fingerprint (`ED25519 SHA256:…`) as its message. The connect form, or the strip over the window for a saved connection, shows it with **Trust this key**; trusting writes the key the server showed into Kemable's file (`trust_ssh_host`) and does again what was asked. The key never crosses to the UI, only its fingerprint.
- Changed (the host is known with another key): `ssh.host_changed`, with no way to trust it from the app. The user fixes the known hosts file, as with `ssh`.

### Secrets

- The SSH password or the key's passphrase goes to Windows Credential Manager as `connection/<id>/ssh` (suffix `.Kemable`), never to `connections.json`.
- It is kept whenever one is given, whatever Save password says: a saved connection's password dialog asks only for the database's, and a tunnel cannot ask at connect time. A blank field when editing keeps the one kept; a connection saved without SSH keeps none.
- Keep-alives every 30 s, three unanswered ending the tunnel, so a NAT or a load balancer does not drop a quiet one.

## Rationale

- **A local port, not a stream handed to each driver**: tiberius, tokio-postgres and the Redis client each want a socket of their own making, and a PostgreSQL cancel request opens a new link from an address the session kept. A port reaches all of them with one field.
- **The host kept for TLS**: rewriting the host to `127.0.0.1` would break SNI (Neon routes on it) and Verify full.
- **Checking keys like OpenSSH**: the user's `known_hosts` already holds the servers they trust; a separate file for Kemable's own keeps the user's file untouched.
- **ring, not aws-lc-rs**: russh's default crypto pulls aws-lc-rs, which builds C with CMake and carries the OpenSSL license; ring is already in the app for rustls.

## Consequences

- Built 2026-11-14: `core/crates/tunnel` with its tests against an SSH server inside them (unknown host, trusted, two links at once, a wrong password, a refused forward, a changed key, a missing key file); `dial` tests in the PostgreSQL and Redis drivers; the SSH secret in `kemable-storage` (tested).
- A connection through SSH holds one SSH session for as long as it is open; each database session is a channel in it.
- No SSH agent and no jump host (`ProxyJump`) yet; a key in PuTTY's `.ppk` form must be converted to OpenSSH first.
- Not tried against a real SSH server here: `docs/testing/phase-3.md` has the steps.

## Alternatives considered

- **Running `ssh.exe -L`**: Windows ships OpenSSH, but its prompts (password, unknown host) would need a console, and its errors are text to parse.
- **One tunnel per session**: a new SSH login for every database session and cancel request is slow, and some bastions limit logins.
- **Trusting any key on first use without asking**: what many clients do; it hides the one moment a man in the middle can be caught.
