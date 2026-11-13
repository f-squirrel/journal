# Security policy

Journal is alpha software and its encryption design has not been independently
audited (see [Storage and security](README.md#storage-and-security)). Reports
are still welcome.

## Reporting a vulnerability

Please do not open a public issue. Use GitHub's private reporting instead:
[Security › Report a vulnerability](https://github.com/f-squirrel/journal/security/advisories/new).

Include what you found, how to reproduce it, and what an attacker could do
with it. This is a personal project, so there is no guaranteed response time,
but reports are read and taken seriously.

## Scope

In scope: the vault and key handling, sync and Dropbox integration, export, and
anything that could leak plaintext or the passphrase.

Out of scope: a stolen device that is already unlocked, a weak passphrase, and
issues in third-party dependencies with no effect on Journal.
