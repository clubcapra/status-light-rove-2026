use std::time::Duration;
use anyhow::{Context, Result};
use serialport::SerialPort;

// ── Hardware byte constants ───────────────────────────────────────────────────

pub const HW_RED_ON:        u8 = 0x11;
pub const HW_RED_OFF:       u8 = 0x21;
pub const HW_RED_BLINK:     u8 = 0x41;

// The physical middle segment is orange, not yellow.
pub const HW_ORANGE_ON:     u8 = 0x12;
pub const HW_ORANGE_OFF:    u8 = 0x22;
pub const HW_ORANGE_BLINK:  u8 = 0x42;

pub const HW_GREEN_ON:      u8 = 0x14;
pub const HW_GREEN_OFF:     u8 = 0x24;
pub const HW_GREEN_BLINK:   u8 = 0x44;

pub const HW_BUZZER_ON:     u8 = 0x18;
pub const HW_BUZZER_OFF:    u8 = 0x28;
pub const HW_BUZZER_BLINK:  u8 = 0x48;

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
    ///
    /// On a write failure the error is returned and the caller is expected to
    /// drop this handle (set the shared `Option<TowerHardware>` to `None`). The
    /// reconnect monitor then reopens the port and replays the logical state, so
    /// reconnection and state restoration are handled in exactly one place
    /// instead of being retried piecemeal here.
    pub fn send(&mut self, cmd: u8) -> Result<()> {
        self.port.write_all(&[cmd])
            .with_context(|| format!("Serial write failed on {}", self.port_path))?;
        Ok(())
    }

    /// Send all-off commands for every channel.
    pub fn all_off(&mut self) -> Result<()> {
        for cmd in [HW_RED_OFF, HW_ORANGE_OFF, HW_GREEN_OFF, HW_BUZZER_OFF] {
            self.send(cmd)?;
        }
        Ok(())
    }
}
