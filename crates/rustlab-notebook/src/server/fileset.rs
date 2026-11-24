//! What `watch` was asked to serve: one file, one directory, or a set of
//! files rooted at their common ancestor directory.

use anyhow::Result;
use std::path::{Path, PathBuf};

use super::http::ServerState;

/// Resolved `watch` inputs. Paths are canonical.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchSet {
    Single(PathBuf),
    Dir(PathBuf),
    Files { root: PathBuf, files: Vec<PathBuf> },
}

pub fn resolve_watch_inputs(_inputs: &[PathBuf]) -> Result<WatchSet> {
    anyhow::bail!("not implemented")
}

pub fn default_jail_root(_set: &WatchSet, _explicit: Option<&Path>) -> Result<Option<PathBuf>> {
    anyhow::bail!("not implemented")
}

pub fn initial_path(_set: &WatchSet, _state: &ServerState) -> String {
    String::new()
}
