//! The managed Arcade apps, straight from Arcade Link's shared metadata
//! (v0.2.0 lists Shelf and Find). Kept as a module so callers have one place
//! to ask.
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
