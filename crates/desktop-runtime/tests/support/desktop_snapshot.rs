// Test-only reads honor Desktop's nonblocking publication contract. This helper
// never submits/replays a command or waits for a model predicate to become true.
use super::desktop_runtime::{Desktop, DesktopError, Snapshot};
use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

#[track_caller]
pub fn read(desktop: &Desktop) -> Arc<Snapshot> {
    read_until(desktop, Instant::now() + Duration::from_secs(5))
}

#[track_caller]
pub fn read_until(desktop: &Desktop, deadline: Instant) -> Arc<Snapshot> {
    loop {
        match desktop.snapshot() {
            Ok(snapshot) => return snapshot,
            Err(DesktopError::Busy) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                assert!(
                    !remaining.is_zero(),
                    "desktop snapshot remained Busy until deadline"
                );
                thread::sleep(Duration::from_millis(2).min(remaining));
            }
            Err(error) => panic!("desktop snapshot failed: {error}"),
        }
    }
}
