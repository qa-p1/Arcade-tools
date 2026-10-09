//! Backward-compatible onboarding metadata while Arcade Link v0.1.0 stays immutable.
//! Remove the Shelf overrides only after a new reviewed Link pin ships them.
use arcade_link::manifest::{self, ids};

pub const SHELF: &str = "arcade.shelf";
pub const APPS: [&str; 6] = [
    ids::BOX,
    ids::LENS,
    ids::LOOK,
    ids::WHEEL,
    ids::CLIPBOARD,
    SHELF,
];
pub fn app_name(id: &str) -> &str {
    if id == SHELF {
        "Arcade Shelf"
    } else {
        manifest::app_name(id)
    }
}
pub fn app_pitch(id: &str) -> &'static str {
    if id == SHELF {
        "Collect, organize and transfer desktop content."
    } else {
        manifest::app_pitch(id)
    }
}
pub fn releases_url(id: &str) -> &'static str {
    if id == SHELF {
        "https://github.com/qa-p1/Arcade-Shelf/releases"
    } else {
        manifest::releases_url(id)
    }
}
