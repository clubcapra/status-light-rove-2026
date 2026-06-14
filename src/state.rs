use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use utoipa::ToSchema;

// ── Per-channel state ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChannelState {
    #[default]
    Off,
    On,
    /// Hardware native blink (fixed ~1 Hz)
    HwBlink,
    /// Software-driven blink: period tracked by BlinkEngine
    SwBlink {
        on_ms:  u64,
        off_ms: u64,
    },
    /// Blink N times then go off
    Pulse {
        on_ms:      u64,
        off_ms:     u64,
        remaining:  u32,
    },
}

impl ChannelState {
    pub fn is_off(&self) -> bool {
        matches!(self, ChannelState::Off)
    }
    pub fn is_active(&self) -> bool {
        !self.is_off()
    }
}

// ── Full light state ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct LightState {
    pub red:          ChannelState,
    /// The physical orange LED segment (labelled "yellow" on the hardware).
    pub orange:       ChannelState,
    pub green:        ChannelState,
    pub buzzer:       ChannelState,
    /// True when the virtual yellow mode is active (red + orange + green
    /// were turned on together). Cleared by any manual per-channel change.
    pub yellow:       bool,
    pub last_updated: Option<DateTime<Utc>>,
}

impl LightState {
    pub fn clear(&mut self) {
        *self = LightState {
            last_updated: Some(Utc::now()),
            ..Default::default()
        };
    }

    /// Set a physical channel state and clear yellow mode, since the user is
    /// now controlling channels independently.
    pub fn set_channel(&mut self, ch: PhysicalChannel, state: ChannelState) {
        self.yellow = false;
        match ch {
            PhysicalChannel::Red    => self.red    = state,
            PhysicalChannel::Orange => self.orange = state,
            PhysicalChannel::Green  => self.green  = state,
            PhysicalChannel::Buzzer => self.buzzer = state,
        }
        self.last_updated = Some(Utc::now());
    }

    /// Set yellow mode state across all three physical channels.
    pub fn set_yellow(&mut self, state: ChannelState, active: bool) {
        self.yellow = active;
        self.red    = state.clone();
        self.orange = state.clone();
        self.green  = state;
        self.last_updated = Some(Utc::now());
    }

    pub fn get_channel(&self, ch: PhysicalChannel) -> &ChannelState {
        match ch {
            PhysicalChannel::Red    => &self.red,
            PhysicalChannel::Orange => &self.orange,
            PhysicalChannel::Green  => &self.green,
            PhysicalChannel::Buzzer => &self.buzzer,
        }
    }
}

// ── Channel enums ─────────────────────────────────────────────────────────────

/// The four physical hardware channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PhysicalChannel {
    Red,
    /// The physical orange LED segment (the middle one on the tower).
    Orange,
    Green,
    Buzzer,
}

impl PhysicalChannel {
    pub fn all() -> [PhysicalChannel; 4] {
        [PhysicalChannel::Red, PhysicalChannel::Orange, PhysicalChannel::Green, PhysicalChannel::Buzzer]
    }
}

impl std::fmt::Display for PhysicalChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PhysicalChannel::Red    => write!(f, "red"),
            PhysicalChannel::Orange => write!(f, "orange"),
            PhysicalChannel::Green  => write!(f, "green"),
            PhysicalChannel::Buzzer => write!(f, "buzzer"),
        }
    }
}

/// A parsed channel from a route — either a physical channel or the virtual yellow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteChannel {
    Physical(PhysicalChannel),
    Yellow,
}

/// Parse a channel name from a route path segment.
pub fn parse_route_channel(s: &str) -> Option<RouteChannel> {
    match s.to_lowercase().as_str() {
        "red"    => Some(RouteChannel::Physical(PhysicalChannel::Red)),
        "orange" => Some(RouteChannel::Physical(PhysicalChannel::Orange)),
        "green"  => Some(RouteChannel::Physical(PhysicalChannel::Green)),
        "buzzer" => Some(RouteChannel::Physical(PhysicalChannel::Buzzer)),
        "yellow" => Some(RouteChannel::Yellow),
        _        => None,
    }
}

// Keep Channel as a type alias for backwards compat with blink_engine
pub type Channel = PhysicalChannel;
