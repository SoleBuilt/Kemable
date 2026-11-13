# Security policy

## Reporting a vulnerability

Report it privately, through
[Report a vulnerability](https://github.com/SoleBuilt/Kemable/security/advisories/new)
or by email to **huynhtrungduc.growth@gmail.com**. Do not open a public issue,
discussion or pull request for a vulnerability.

Include:

- What is affected: the Windows app, or the shared Rust core (`core/`), with its
  version or the commit you built from.
- Your Windows version, and the database engine and version when a driver is
  involved.
- Steps to reproduce, or a proof of concept.
- What an attacker gains: the data or the access exposed.

Write in English or Vietnamese. Leave real passwords, connection strings and
private data out of the report.

## Scope

- The Windows app, its installer, and the data it keeps: the data folder, and
  the passwords in Windows Credential Manager.
- The shared Rust core and its drivers.
- This repository's workflows.

## Supported versions

Kemable has no release yet. Reports about the latest commit on `main` are
welcome. Once releases start, the latest one gets security fixes, and this file
will say so.
