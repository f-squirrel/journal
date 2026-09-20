//! ONE-SHOT REPAIR: give entries written before `entry_date` existed their
//! `updated_at` as an entry date.
//!
//! Run it once, then delete it along with `src/backfill.rs`. Nothing in the
//! app calls any of this — see that module's header for why a separate tool
//! is not the compatibility shim `CLAUDE.md` forbids.
//!
//! Usage:
//!   cargo run --release -p journal-vault --example backfill -- <entries-dir> [--apply]
//!
//! Without `--apply` it only counts what it would change. The passphrase is
//! read from stdin, never from the command line — an argument is visible to
//! anyone who can run `ps` — and is never printed or written anywhere.
//!
//! **Quit the journal app first.** This writes to the same blobs the running
//! app has open, and a sync pass mid-run would upload half a repair.

use std::io::{IsTerminal, Read};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use journal_fs_store::FsStore;
use journal_vault::{BackfillMode, Vault};
use secrecy::SecretString;

/// The sealed key, which is what tells a journal directory from any other.
/// Named here rather than imported because `journal-key` keeps its own
/// constant private; if they ever disagree the check below simply refuses a
/// directory it should have accepted, which is the safe direction.
const KEY_BLOB: &str = ".vault-key";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(dir) = args.next().map(PathBuf::from) else {
        eprintln!(
            "usage: cargo run --release -p journal-vault --example backfill \
             -- <entries-dir> [--apply]"
        );
        return ExitCode::FAILURE;
    };
    let apply = args.any(|a| a == "--apply");

    // `Vault::open` creates a journal in an empty store, which is right for
    // the app and catastrophic here: point this at the wrong path and it
    // would drop a fresh key into it. A directory holding no sealed key is
    // not the one the user meant.
    if !dir.join(KEY_BLOB).exists() {
        eprintln!(
            "{}: no {KEY_BLOB} here — is that the entries directory?",
            dir.display()
        );
        return ExitCode::FAILURE;
    }

    if std::io::stdin().is_terminal() {
        eprintln!("Enter the journal passphrase, then Ctrl-D:");
    }
    let mut passphrase = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut passphrase) {
        eprintln!("could not read the passphrase: {e}");
        return ExitCode::FAILURE;
    }
    // A trailing newline from the terminal is not part of what was typed.
    let passphrase = SecretString::from(passphrase.trim_end_matches(['\n', '\r']).to_owned());

    let store = Arc::new(FsStore::new(dir));
    let vault = match Vault::open(store, &passphrase) {
        Ok(vault) => vault,
        Err(e) => {
            eprintln!("could not unlock: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mode = if apply {
        BackfillMode::Apply
    } else {
        BackfillMode::DryRun
    };
    let report = match vault.backfill_entry_dates(mode) {
        Ok(report) => report,
        Err(e) => {
            eprintln!("backfill failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Counts only: an entry's title or id on a terminal is journal content.
    println!("entries scanned:     {}", report.scanned);
    println!(
        "{} {}",
        if apply {
            "given an entry date:"
        } else {
            "would be repaired:  "
        },
        report.backfilled
    );
    println!("already dated:       {}", report.already_dated);
    if report.unreadable > 0 {
        println!("unreadable, skipped: {}", report.unreadable);
    }
    if !apply && report.backfilled > 0 {
        println!("\nNothing was written. Re-run with --apply to write it.");
    }
    if apply && report.backfilled > 0 {
        println!("\nDone. Start the app; if sync is connected, these will upload.");
    }
    ExitCode::SUCCESS
}
