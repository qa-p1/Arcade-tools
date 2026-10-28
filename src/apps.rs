//! Compiled app IDs and installation knowledge from the pinned Arcade Link.
//! `catalog` controls manager membership and display metadata. Keep these IDs,
//! names (native paths), and the per-app contracts in `paths`/`platform` until
//! release manifests can describe their installation specifics safely.
use arcade_link::manifest::{self, ids};

pub const SHELF: &str = ids::SHELF;
pub const FIND: &str = ids::FIND;
pub const APPS: [&str; 7] = ids::APPS;
pub fn app_name(id: &str) -> &str {
    manifest::app_name(id)
}
pub fn app_pitch(id: &str) -> &'static str {
    manifest::app_pitch(id)
}
pub fn releases_url(id: &str) -> &'static str {
    manifest::releases_url(id)
}
