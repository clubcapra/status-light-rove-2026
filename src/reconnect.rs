use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, warn};

use crate::hardware::TowerHardware;
use crate::state::{ChannelState, LightState, PhysicalChannel};
use crate::AppState;

/// How often the monitor probes for a reappeared device while disconnected.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Push the current logical light state onto a freshly opened device so the
/// physical LEDs match what the API believes is showing.
///
/// Software-driven effects (sw-blink / pulse / timed / sequence) are driven by
/// live `BlinkEngine` tasks that share the same hardware handle and resume on
/// their next cycle, so here we only re-assert the static states (`On` /
/// `HwBlink`). `Off` is re-sent to guarantee a clean baseline on a device that
/// may have powered up in an unknown state.
pub fn restore_light_state(light: &LightState, hw: &mut TowerHardware) {
    for ch in PhysicalChannel::all() {
        let (on, off, blink) = ch.hw_commands();
        let cmd = match light.get_channel(ch) {
            ChannelState::Off     => off,
            ChannelState::On      => on,
            ChannelState::HwBlink => blink,
            // Driven by a live blink task; it resumes on its own once the
            // handle is published again, so don't touch it here.
            ChannelState::SwBlink { .. } | ChannelState::Pulse { .. } => continue,
        };
        if let Err(e) = hw.send(cmd) {
            warn!("state restore: failed to re-send {ch}: {e}");
            return;
        }
    }
}

/// Background task that keeps the hardware handle and the physical LEDs in sync
/// with the logical state across USB unplug/replug.
///
/// While a live handle is held it does nothing — disconnects surface as write
/// errors that reset the handle to `None` (in route handlers and the blink
/// engine). Once disconnected, it polls for the device and, on the first
/// successful reopen, replays the logical state before publishing the handle so
/// blink tasks don't race in mid-restore.
pub async fn run_reconnect_monitor(s: AppState) {
    loop {
        sleep(POLL_INTERVAL).await;

        // Fast path: we already hold a handle, nothing to do.
        if s.hw.lock().await.is_some() {
            continue;
        }

        let Some(port) = crate::routes::resolve_port(&s) else { continue };
        let mut dev = match TowerHardware::open(&port) {
            Ok(d) => d,
            Err(_) => continue, // not plugged back in yet
        };

        // Re-check under the lock in case a request reconnected first, then
        // restore state before making the handle visible to the blink tasks.
        let mut hw = s.hw.lock().await;
        if hw.is_some() {
            continue;
        }
        {
            let light = s.light.lock().await;
            restore_light_state(&light, &mut dev);
        }
        info!("Tower light reconnected on {port}; state restored");
        *hw = Some(dev);
    }
}
