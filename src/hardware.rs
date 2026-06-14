use std::time::Duration;
use anyhow::{Context, Result};
use serialport::SerialPort;
use tracing::{info, warn, error};

// ── Hardware byte constants ───────────────────────────────────────────────────

pub const HW_RED_ON:       u8 = 0x11;
pub const HW_RED_OFF:      u8 = 0x21;
pub const HW_RED_BLINK:    u8 = 0x41;

pub const HW_YELLOW_ON:    u8 = 0x12;
pub const HW_YELLOW_OFF:   u8 = 0x22;
pub const HW_YELLOW_BLINK: u8 = 0x42;

pub const HW_GREEN_ON:     u8 = 0x14;
pub const HW_GREEN_OFF:    u8 = 0x24;
pub const HW_GREEN_BLINK:  u8 = 0x44;

pub const HW_BUZZER_ON:    u8 = 0x18;
pub const HW_BUZZER_OFF:   u8 = 0x28;
pub const HW_BUZZER_BLINK: u8 = 0x48;

/// CH340 USB VID/PID used by the Adafruit tower light.
pub const TOWER_VID: u16 = 0x1a86;
pub const TOWER_PID: u16 = 0x7523;

// ── USB enumeration ───────────────────────────────────────────────────────────

/// Scan available serial ports and return the path of the first one that
/// matches the given VID/PID. Returns None if no matching device is found.
pub fn find_device_port(vid: u16, pid: u16) -> Option<String> {
    let ports = serialport::available_ports().ok()?;
    for p in &ports {
        if let serialport::SerialPortType::UsbPort(usb) = &p.port_type {
            if usb.vid == vid && usb.pid == pid {
                return Some(p.port_name.clone());
            }
        }
    }
    None
}

// ── TowerHardware ─────────────────────────────────────────────────────────────

pub struct TowerHardware {
    port:      Box<dyn SerialPort>,
    port_path: String,
}

impl TowerHardware {
    pub fn open(path: &str) -> Result<Self> {
        let port = serialport::new(path, 9600)
            .timeout(Duration::from_millis(200))
            .open()
            .with_context(|| format!("Failed to open serial port {path}"))?;
        Ok(Self {
            port,
            port_path: path.to_string(),
        })
    }

    /// Send a single command byte to the hardware.
    /// On failure, attempt one reconnect then retry.
    /// Returns Err if the device is truly gone after the reconnect attempt.
    pub fn send(&mut self, cmd: u8) -> Result<()> {
        if let Err(e) = self.port.write_all(&[cmd]) {
            warn!("Serial write failed ({e}), attempting reconnect...");
            self.reconnect()?;
            self.port.write_all(&[cmd])
                .context("Write failed after reconnect")?;
        }
        Ok(())
    }

    /// Send all-off commands for every channel.
    pub fn all_off(&mut self) -> Result<()> {
        for cmd in [HW_RED_OFF, HW_YELLOW_OFF, HW_GREEN_OFF, HW_BUZZER_OFF] {
            self.send(cmd)?;
        }
        Ok(())
    }

    /// Attempt to reopen the serial port (called after a write error).
    fn reconnect(&mut self) -> Result<()> {
        std::thread::sleep(Duration::from_millis(500));
        match serialport::new(&self.port_path, 9600)
            .timeout(Duration::from_millis(200))
            .open()
        {
            Ok(p) => {
                self.port = p;
                info!("Reconnected to {}", self.port_path);
                Ok(())
            }
            Err(e) => {
                error!("Reconnect failed: {e}");
                Err(anyhow::anyhow!(e))
            }
        }
    }
}