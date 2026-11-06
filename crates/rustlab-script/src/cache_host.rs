//! Process-wide permission for the SQLite persistent function cache.
//!
//! `cache enable` opens or creates a store only when this policy allows
//! it. The CLI and notebook binaries set the policy from `--cache` /
//! `--no-cache` and `[cache] enabled` before they build an evaluator.
//! A library [`crate::Evaluator`] with the policy unset still allows
//! `cache enable`, so in-process tests keep working. Notebook execution
//! treats an unset policy as deny, so a render does not create
//! `cache.db` unless the binary opted in.

use std::sync::atomic::{AtomicU8, Ordering};

const UNSET: u8 = 0;
const DENY: u8 = 1;
const ALLOW: u8 = 2;

static HOST: AtomicU8 = AtomicU8::new(UNSET);

/// Install the host policy. `None` restores the unset (library) state.
/// `Some(true)` lets `cache enable` open a store. `Some(false)` refuses
/// it before any filesystem access.
pub fn set_host_persistent_cache(permitted: Option<bool>) {
    let v = match permitted {
        None => UNSET,
        Some(false) => DENY,
        Some(true) => ALLOW,
    };
    HOST.store(v, Ordering::Relaxed);
}

/// `Some(true)` / `Some(false)` when a host set the policy, `None` when unset.
pub fn host_persistent_cache() -> Option<bool> {
    match HOST.load(Ordering::Relaxed) {
        ALLOW => Some(true),
        DENY => Some(false),
        _ => None,
    }
}

/// CLI / config resolution. An explicit flag wins over the rc key.
/// `cli` is `Some(true)` for `--cache` and `Some(false)` for `--no-cache`.
pub fn resolve_persistent_cache(cli: Option<bool>, config_enabled: bool) -> bool {
    cli.unwrap_or(config_enabled)
}
