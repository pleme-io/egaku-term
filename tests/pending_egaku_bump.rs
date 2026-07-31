//! The `pending-egaku-bump` seal — a pending state must not outlive its cause.
//!
//! # What is and is not enforced here
//!
//! The *forward* direction — "the bump was forgotten, so this crate is broken"
//! — needs no test and gets none. It is already sealed at the strongest tier
//! available: with the locked egaku, `src/draw.rs`'s `use egaku::{… TableView}`
//! is `E0432 unresolved import`, so **the artifact cannot be produced at all**.
//! A test cannot improve on a hard compile error, and a detector layered on top
//! of one is theatre.
//!
//! What is *not* sealed by the compiler is the opposite direction: once the
//! bump lands, everything compiles, everything is green, and the
//! `pending-egaku-bump:` block at the top of `CLAUDE.md` still announces
//! "this repo does not compile against its currently-locked egaku" — a claim
//! that is now false and that nothing re-reads. That is the rot this file
//! catches, and it is the direction a human is actually able to forget.
//!
//! # How the two compose
//!
//! The compiler proves the *capability* (a lock naming an egaku without
//! `TableView` never reaches a test). This file proves the *doc matches the
//! lock*. Neither alone closes the class; together, the pending token can
//! neither outlive its cause nor be dropped while the cause stands.
//!
//! # Reachability, stated honestly
//!
//! | `Cargo.lock` says | compiles? | this file |
//! |---|---|---|
//! | `git+…#507b0e7…` (today, committed) | no | never runs — the compiler is the gate |
//! | no `source` (a `[patch]` path override) | yes | **neutral** — see below |
//! | `git+…#<any other rev>` / crates.io | yes | **fires**: the token must be gone |
//!
//! The path-override row is deliberately neutral. That state is ambiguous —
//! it is produced both by today's measurement workflow (`cargo test --config
//! 'patch."https://github.com/pleme-io/egaku".egaku.path="../egaku"'`) and, long
//! after this token clears, by anyone doing ordinary local egaku development.
//! Asserting anything about the token there would fire falsely forever on the
//! second case, so the arm asserts nothing and says so.
//!
//! The seal is therefore a **no-op today** and arms itself at exactly the
//! moment the pending state clears. That is the design, not a shortfall.
//!
//! # Not re-implemented here
//!
//! The "regenerate `Cargo.gen.lock` in the same commit" step of the chain is
//! already a **hard Nix eval throw** in substrate's `lib/build/rust/
//! lockfile-delta.nix` (the D2 freshness tie: the delta records
//! `sha256(Cargo.lock)`). Duplicating it in Rust would be a second, weaker
//! home for one invariant. Run `gen confirm` for the offline operator view.

use std::fs;
use std::path::Path;

/// The egaku commit `Cargo.lock` pins today — the last one *before*
/// `Selectable` / `TableView` / the widget-owned focus flags.
const STALE_REV: &str = "507b0e7be84568bbb5567b2d61f5ce9ab6501882";

/// The waiver token, spelled exactly as `CLAUDE.md` carries it.
const TOKEN: &str = "pending-egaku-bump:";

/// What `Cargo.lock` currently says egaku is. Total over `Option<&str>`, so a
/// new lock shape cannot fall through every arm and read as "fine" — the
/// silent-third-event failure mode substrate's own gen-spec workflow was
/// bitten by.
#[derive(Debug, PartialEq, Eq)]
enum Locked {
    /// No `source` key: egaku resolves to a local path, so a `[patch]`
    /// override is in effect and the committed lock is not what is being
    /// compiled. Ambiguous — asserts nothing.
    PathOverride,
    /// Still the pre-`TableView` commit. Unreachable from a running test
    /// (that lock does not compile); kept so the match is total.
    StaleGitRev,
    /// Any other egaku — a bumped git rev, or a crates.io release. The pin
    /// this token exists for is gone.
    Bumped(String),
}

fn classify(source: Option<&str>) -> Locked {
    match source {
        None => Locked::PathOverride,
        Some(s) => match s.split_once("#") {
            Some((_, rev)) if s.starts_with("git+") && rev == STALE_REV => Locked::StaleGitRev,
            Some((_, rev)) if s.starts_with("git+") => Locked::Bumped(rev.to_string()),
            _ => Locked::Bumped(s.to_string()),
        },
    }
}

/// `key = "value"` on one already-trimmed `Cargo.lock` line.
fn kv(line: &str, key: &str) -> Option<String> {
    let rest = line.strip_prefix(key)?.trim_start().strip_prefix('=')?.trim();
    Some(rest.strip_prefix('"')?.strip_suffix('"')?.to_string())
}

/// The egaku `[[package]]` entry: `(version, source)`. `None` when the lock
/// carries no egaku at all — which this file treats as a failure, never as
/// "nothing to check".
fn egaku_entry(lock: &str) -> Option<(String, Option<String>)> {
    for block in lock.split("[[package]]") {
        let (mut name, mut version, mut source) = (None, None, None);
        for line in block.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                break; // a following table — this package block has ended
            }
            if let Some(v) = kv(line, "name") {
                name = Some(v);
            } else if let Some(v) = kv(line, "version") {
                version = Some(v);
            } else if let Some(v) = kv(line, "source") {
                source = Some(v);
            }
        }
        if name.as_deref() == Some("egaku") {
            return Some((version.unwrap_or_default(), source));
        }
    }
    None
}

/// The executable clearing chain. Every step verified against this repo's
/// actual files — in particular step 2, which is *not* the obvious one.
const CHAIN: &str = "\
  1. cd ../egaku && git push origin main
     Commit e602369 (`the first two traits — Selectable, and TableView lifted
     from banken`) is LOCAL-ONLY. Nothing below can work until it is pushed;
     a Cargo.lock naming an unfetchable rev is worse than one naming a stale
     rev, which is why the bump was withheld rather than guessed.

  2. cd ../egaku-term && cargo update -p egaku
     NOTE — there is NO rev to edit. Cargo.toml carries
     `egaku = { git = \"https://github.com/pleme-io/egaku\", version = \"0.1\" }`
     with no `rev =` key, so the manifest does not change at all; `cargo
     update` alone moves the locked rev to the new default-branch head.

  3. gen build . --commit
     Regenerates Cargo.gen.lock. The delta records sha256(Cargo.lock) and
     substrate's lib/build/rust/lockfile-delta.nix THROWS at eval when the two
     disagree (the D2 freshness tie), so this is same-commit-mandatory, not
     tidy-up. Check it offline with `gen confirm`.

  4. cargo test
     With NO `--config patch` override — that is the entire point of the bump.

  5. Delete the `pending-egaku-bump:` block at the top of CLAUDE.md.

  6. Commit steps 2-5 TOGETHER (Cargo.lock + Cargo.gen.lock + CLAUDE.md).";

#[test]
fn the_pending_token_cannot_outlive_the_locked_egaku() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));

    // Both reads are hard failures, never skips. A guard that tolerates a
    // missing input is a guard that examined nothing, and would let this
    // whole file pass green forever without reading a byte.
    let lock = fs::read_to_string(root.join("Cargo.lock"))
        .expect("Cargo.lock must exist beside Cargo.toml — this seal reads it, it cannot skip");
    let claude = fs::read_to_string(root.join("CLAUDE.md"))
        .expect("CLAUDE.md must exist — it is where the pending token lives");

    let (version, source) = egaku_entry(&lock).expect(
        "no `egaku` [[package]] entry in Cargo.lock — either the dependency was \
         removed (then delete this file and the pending token together) or the \
         lock parser above no longer matches cargo's format",
    );

    match classify(source.as_deref()) {
        // Ambiguous by construction — see the module docs. Asserts nothing.
        Locked::PathOverride => {}

        // Unreachable while `draw.rs` needs `TableView`: this lock does not
        // compile, so no test binary exists to run. Kept for totality.
        Locked::StaleGitRev => {
            assert!(
                claude.contains(TOKEN),
                "Cargo.lock still pins egaku {version} @ {STALE_REV}, which predates \
                 Selectable/TableView, but CLAUDE.md no longer carries `{TOKEN}`. \
                 The warning was removed while the condition still holds.\n\n{CHAIN}"
            );
        }

        Locked::Bumped(rev) => {
            assert!(
                !claude.contains(TOKEN),
                "STALE WAIVER. Cargo.lock now resolves egaku {version} from `{rev}`, \
                 not the pre-TableView {STALE_REV} — so the bump has LANDED and \
                 CLAUDE.md's `{TOKEN}` block is a false claim that this repo does \
                 not compile against its locked egaku. Delete that block.\n\n\
                 For reference, the chain it was recording:\n\n{CHAIN}"
            );
        }
    }
}

/// The red run against deliberately-broken input (★★ UNREPRESENTABILITY: a
/// gate you add records one red run). The seal's decision is a pure function,
/// so its firing can be proven here without corrupting the real `Cargo.lock`
/// — which, in the stale state, could not be compiled to run this test anyway.
#[test]
fn the_seal_fires_on_a_bumped_lock_and_stays_quiet_on_the_override() {
    // Total over the four shapes a lock can carry.
    assert_eq!(classify(None), Locked::PathOverride);
    assert_eq!(
        classify(Some(
            "git+https://github.com/pleme-io/egaku#507b0e7be84568bbb5567b2d61f5ce9ab6501882"
        )),
        Locked::StaleGitRev
    );
    assert_eq!(
        classify(Some("git+https://github.com/pleme-io/egaku#deadbeefdeadbeef")),
        Locked::Bumped("deadbeefdeadbeef".to_string())
    );
    assert_eq!(
        classify(Some("registry+https://github.com/rust-lang/crates.io-index")),
        Locked::Bumped("registry+https://github.com/rust-lang/crates.io-index".to_string())
    );

    // The parser reads a real lock shape, both states, and finds the entry.
    let stale = "\
[[package]]
name = \"crossterm\"
version = \"0.28.1\"
source = \"registry+https://github.com/rust-lang/crates.io-index\"

[[package]]
name = \"egaku\"
version = \"0.1.3\"
source = \"git+https://github.com/pleme-io/egaku#507b0e7be84568bbb5567b2d61f5ce9ab6501882\"
dependencies = [
 \"serde\",
]
";
    let (v, s) = egaku_entry(stale).expect("must find the egaku entry");
    assert_eq!(v, "0.1.3");
    assert_eq!(classify(s.as_deref()), Locked::StaleGitRev);

    let patched = stale
        .replace(
            "source = \"git+https://github.com/pleme-io/egaku#507b0e7be84568bbb5567b2d61f5ce9ab6501882\"\n",
            "",
        )
        .replace("0.1.3", "0.1.4");
    let (v, s) = egaku_entry(&patched).expect("must find the path-override egaku entry");
    assert_eq!(v, "0.1.4");
    assert_eq!(classify(s.as_deref()), Locked::PathOverride);

    // A lock with no egaku at all is a failure, not a pass.
    assert!(egaku_entry("[[package]]\nname = \"serde\"\nversion = \"1\"\n").is_none());

    // And the assertion the live test makes actually discriminates: the token
    // present under a Bumped lock is the violation, absent is clean.
    let claude_with = "> pending-egaku-bump: Draw + draw::table need egaku >= ...";
    let claude_without = "# Egaku-Term\n\nno waiver here";
    assert!(claude_with.contains(TOKEN), "red input must be detected");
    assert!(!claude_without.contains(TOKEN), "clean input must pass");
}
