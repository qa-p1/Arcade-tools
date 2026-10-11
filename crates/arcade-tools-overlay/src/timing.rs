use std::time::Instant;

#[derive(Clone)]
pub struct Timing {
    start: Instant,
    enabled: bool,
}

impl Timing {
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
            enabled: std::env::var_os("ARCADE_OVERLAY_TIMING").is_some_and(|v| v == "1"),
        }
    }

    pub fn phase(&self, name: &str, began: Instant) {
        if self.enabled {
            eprintln!(
                "arcade-overlay phase={name} duration_ms={:.3} elapsed_ms={:.3}",
                began.elapsed().as_secs_f64() * 1000.0,
                self.start.elapsed().as_secs_f64() * 1000.0
            );
        }
    }

    pub fn first_frame(&self, backend: &str) {
        if self.enabled {
            eprintln!(
                "arcade-overlay first_frame backend={backend} spawn_to_first_frame_ms={:.3}",
                self.start.elapsed().as_secs_f64() * 1000.0
            );
        }
    }
}
