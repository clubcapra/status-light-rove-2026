mod hardware;
mod state;
mod routes;
mod blink_engine;
mod reconnect;

use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, error};
use anyhow::Result;

use hardware::{TowerHardware, find_device_port, TOWER_VID, TOWER_PID};
use state::LightState;
use blink_engine::BlinkEngine;

// ── Shared application state ─────────────────────────────────────────────────

#[derive(Clone)]
pub struct AppState {
    /// None when the hardware device is not connected / failed to open.
    pub hw:      Arc<Mutex<Option<TowerHardware>>>,
    pub light:   Arc<Mutex<LightState>>,
    pub blinker: Arc<BlinkEngine>,
    /// USB VID/PID used to find the device on enumeration.
    pub vid:     u16,
    pub pid:     u16,
    /// Optional CLI-override port path. When set, skip VID/PID enumeration
    /// and always try this path directly.
    pub port_override: Option<String>,
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "tower_api=info,axum=warn".into()),
        )
        .init();

    // If a port path is given on the CLI, use it directly instead of
    // enumerating by VID/PID. This covers non-CH340 adapters or fixed paths.
    let port_override = std::env::args().nth(1);

    // Try to open hardware on startup — failure is non-fatal, API still starts.
    let initial_port = port_override.clone()
        .or_else(|| find_device_port(TOWER_VID, TOWER_PID));

    let hw_device = match initial_port {
        None => {
            error!(
                "Tower light not found on USB (VID={:#06x} PID={:#06x}).\n\
                 API will start without hardware — all control endpoints \
                 will return 503 until the device is plugged in.",
                TOWER_VID, TOWER_PID
            );
            None
        }
        Some(ref path) => match TowerHardware::open(path) {
            Ok(hw) => {
                info!("Tower light connected on {path}");
                Some(hw)
            }
            Err(e) => {
                error!("Found device at {path} but failed to open it: {e}");
                None
            }
        },
    };

    let light = LightState::default();
    let hw    = Arc::new(Mutex::new(hw_device));
    let light = Arc::new(Mutex::new(light));

    // Boot sequence: only run if hardware is present.
    {
        let mut hw_lock = hw.lock().await;
        if let Some(ref mut hw_dev) = *hw_lock {
            let mut light_lock = light.lock().await;
            if let Err(e) = hw_dev.all_off() {
                error!("Boot all_off failed: {e}");
            }
            light_lock.clear();
            if let Err(e) = hw_dev.send(hardware::HW_GREEN_ON) {
                error!("Boot green_on failed: {e}");
            }
            light_lock.green = state::ChannelState::On;
            info!("Boot state: GREEN ON");
        } else {
            info!("Boot state: no hardware, skipping boot sequence");
        }
    }

    let blinker = Arc::new(BlinkEngine::new(Arc::clone(&hw), Arc::clone(&light)));

    let app_state = AppState {
        hw,
        light,
        blinker,
        vid: TOWER_VID,
        pid: TOWER_PID,
        port_override,
    };

    // Keep the physical LEDs in sync with the logical state across USB
    // unplug/replug, even when no requests are arriving.
    tokio::spawn(reconnect::run_reconnect_monitor(app_state.clone()));

    let router = routes::build_router(app_state);

    let bind = std::env::var("TOWER_BIND").unwrap_or_else(|_| "0.0.0.0:3000".into());
    info!("Tower light API listening on http://{}", bind);
    eprintln!("✔ Tower light API ready → http://{}", bind);

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    axum::serve(listener, router).await?;

    Ok(())
}
