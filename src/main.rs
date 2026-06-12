mod hardware;
mod state;
mod routes;
mod blink_engine;

use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};
use anyhow::Result;

use hardware::TowerHardware;
use state::LightState;
use blink_engine::BlinkEngine;

// ── USB autodetection ────────────────────────────────────────────────────────

/// Walk available serial ports and return the first one that looks like the
/// CH340 tower light (vendor 0x1a86, product 0x7523).
fn autodetect_port() -> Option<String> {
    let ports = serialport::available_ports().ok()?;
    for p in &ports {
        if let serialport::SerialPortType::UsbPort(usb) = &p.port_type {
            // QinHeng CH340 — the chip used by the Adafruit tower light
            if usb.vid == 0x1a86 && usb.pid == 0x7523 {
                info!("Autodetected tower light on {}", p.port_name);
                return Some(p.port_name.clone());
            }
        }
    }
    // Fallback: if exactly one USB serial port exists, assume it's ours
    let usb_ports: Vec<_> = ports
        .iter()
        .filter(|p| matches!(p.port_type, serialport::SerialPortType::UsbPort(_)))
        .collect();
    if usb_ports.len() == 1 {
        warn!(
            "No CH340 match; falling back to only USB serial port: {}",
            usb_ports[0].port_name
        );
        return Some(usb_ports[0].port_name.clone());
    }
    None
}

// ── Shared application state ─────────────────────────────────────────────────

#[derive(Clone)]
pub struct AppState {
    pub hw:     Arc<Mutex<TowerHardware>>,
    pub light:  Arc<Mutex<LightState>>,
    pub blinker: Arc<BlinkEngine>,
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

    // Port resolution: CLI arg → autodetect → udev symlink fallback
    let port_path = std::env::args()
        .nth(1)
        .or_else(autodetect_port)
        .unwrap_or_else(|| {
            warn!("Could not autodetect port; trying /dev/tower-light (udev symlink)");
            "/dev/tower-light".to_string()
        });

    info!("Opening serial port: {}", port_path);

    let hw = TowerHardware::open(&port_path)?;
    let light = LightState::default();

    let hw    = Arc::new(Mutex::new(hw));
    let light = Arc::new(Mutex::new(light));

    // Boot sequence: clear any stale state, then set yellow to signal "starting up"
    {
        let mut hw_lock    = hw.lock().await;
        let mut light_lock = light.lock().await;
        hw_lock.all_off()?;
        light_lock.clear();
        hw_lock.send(hardware::HW_YELLOW_ON)?;
        light_lock.yellow = state::ChannelState::On;
        info!("Boot state: YELLOW ON");
    }

    let blinker = Arc::new(BlinkEngine::new(Arc::clone(&hw), Arc::clone(&light)));

    let app_state = AppState { hw, light, blinker };

    let router = routes::build_router(app_state);

    let bind = std::env::var("TOWER_BIND").unwrap_or_else(|_| "0.0.0.0:3000".into());
    info!("Tower light API listening on http://{}", bind);

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    axum::serve(listener, router).await?;

    Ok(())
}
