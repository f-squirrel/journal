# 0016. Define settings as a device struct and a journal struct, merged per field

## Status

Accepted — 2026-09-27

## Decision

Settings are defined in Rust, in the `journal-settings` crate, as two
structs. `DeviceSettings` belongs to this device only: it is plaintext JSON
in `app_config_dir()/device-settings.json`, never syncs, and can be read
while the vault is locked. `JournalSettings` belongs to the journal: it is
stored as the encrypted `.journal-settings` blob and syncs with the entries.
The frontend sees them as one flat `Settings` (both structs
`#[serde(flatten)]`ed) and writes back patches that name only the fields
that changed. The backend routes each field to the struct that declares it.

The journal blob stores each field with a `changed_at` timestamp. On a sync
conflict, the resolver merges it per field: the later stamp wins, with a
deterministic tie-break, and keys only one side knows are kept. A local
change is stamped `max(now, previous + 1 ms)`. Updates run under the vault's
`settings_lock`, and the merge re-reads the stored blob, so a save and a merge
cannot erase each other.

## Reasons

- The struct a field sits on decides whether it syncs, so the compiler
  enforces that each setting has exactly one home. Adding a setting means
  adding a field with a default.
- The resolver gets only "ours" and "theirs", not the common base. Stamping
  each field makes two devices that changed different settings both keep
  their change, which whole-document last-writer-wins would not.
- Patches keep a copy read while locked (journal half as defaults) from
  overwriting real values, and tell "set to never" (`null`) apart from
  "leave alone" (a missing key).
- JSON everywhere (ADR 0003) allows per-field tolerant decoding: a bad or
  unknown stored field falls back to its default alone, with no migration
  code.

## Alternatives considered

- **Two separate APIs and contexts** (device and journal): four commands, a
  TS copy of the journal defaults, and moving a setting between scopes
  becomes a frontend change.
- **One struct plus a table of scopes:** a field missing from the table only
  shows up at runtime.
- **Whole-document last-writer-wins:** silently loses a concurrent change to
  a different field.
- **Three-way merge against the sync base:** would change the
  `ConflictResolver` contract (ADR 0012) to serve one blob.
- **Settings inside the `.vault-key` blob:** that blob is never merged, and
  changing it halts sync on other devices.
- **TOML or a binary format:** a new dependency, or not self-describing.
