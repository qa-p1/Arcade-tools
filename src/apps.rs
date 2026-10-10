//! Backward-compatible onboarding metadata while Arcade Link v0.1.0 stays immutable.
//! Remove the Shelf and Find overrides only after a new reviewed Link pin ships them.
use arcade_link::manifest::{self, ids};

pub const SHELF: &str = "arcade.shelf";
pub const FIND: &str = "arcade.find";
pub const APPS: [&str; 7] = [
    ids::BOX,
    ids::LENS,
    ids::LOOK,
    ids::WHEEL,
    ids::CLIPBOARD,
    SHELF,
    FIND,
];
pub fn app_name(id: &str) -> &str {
    match id {
        SHELF => "Arcade Shelf",
        FIND => "Arcade Find",
        _ => manifest::app_name(id),
    }
}
pub fn app_pitch(id: &str) -> &'static str {
    match id {
        SHELF => "Collect, organize and transfer desktop content.",
        FIND => "Find files and folders instantly.",
        _ => manifest::app_pitch(id),
    }
}
pub fn releases_url(id: &str) -> &'static str {
    match id {
        SHELF => "https://github.com/qa-p1/Arcade-Shelf/releases",
        FIND => "https://github.com/qa-p1/Arcade-Find/releases",
        _ => manifest::releases_url(id),
    }
}
