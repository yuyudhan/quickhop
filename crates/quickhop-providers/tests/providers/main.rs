//! Provider integration tests, one binary over the fixtures in `tests/fixtures`:
//! - [`contract`]: the contract every provider passes;
//! - [`apps`]: desktop entries, icon themes, `.app` bundles, Start Menu shortcuts;
//! - [`scripts`]: Raycast script-command discovery and execution;
//! - [`snapshots`]: insta snapshots of ranked result lists.

mod apps;
mod common;
mod contract;
mod scripts;
mod snapshots;
