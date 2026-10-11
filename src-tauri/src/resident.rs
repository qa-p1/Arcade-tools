//! Window lifetime decisions, independent of Tauri and the tray backend.

mod memory;
pub use memory::release_unused_runtime_pages;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Close {
    DestroyWindow,
    Quit,
    Busy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Open {
    CreateWindow,
    ShowWindow,
}

#[derive(Debug, Clone, Copy)]
pub struct Resident {
    pub hosting: bool,
    pub shortcuts: bool,
    window: bool,
}

impl Resident {
    pub fn new(background: bool) -> Self {
        Self {
            hosting: true,
            shortcuts: true,
            window: !background,
        }
    }

    pub fn keeps_running(&self) -> bool {
        self.hosting || self.shortcuts
    }

    pub fn close(&mut self, busy: bool) -> Close {
        if self.keeps_running() {
            self.window = false;
            Close::DestroyWindow
        } else if busy {
            Close::Busy
        } else {
            self.window = false;
            Close::Quit
        }
    }

    pub fn open(&mut self) -> Open {
        let action = if self.window {
            Open::ShowWindow
        } else {
            Open::CreateWindow
        };
        self.window = true;
        action
    }

    pub fn destroyed(&mut self) {
        self.window = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_destroys_reopen_creates_and_background_has_no_window() {
        let mut state = Resident::new(false);
        assert_eq!(state.open(), Open::ShowWindow);
        assert_eq!(state.close(true), Close::DestroyWindow);
        assert_eq!(state.open(), Open::CreateWindow);
        assert_eq!(Resident::new(true).open(), Open::CreateWindow);
        state.destroyed();
        assert_eq!(state.open(), Open::CreateWindow);
    }

    #[test]
    fn either_feature_keeps_running_and_quit_respects_busy() {
        let mut state = Resident::new(false);
        state.hosting = false;
        assert_eq!(state.close(false), Close::DestroyWindow);
        state.shortcuts = false;
        assert_eq!(state.close(true), Close::Busy);
        assert_eq!(state.close(false), Close::Quit);
        state.hosting = true;
        assert!(state.keeps_running());
    }
}
