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
    pub yellow:       ChannelState,
    pub green:        ChannelState,
    pub buzzer:       ChannelState,
    pub last_updated: Option<DateTime<Utc>>,
}

impl LightState {
    pub fn clear(&mut self) {
        *self = LightState {
            last_updated: Some(Utc::now()),
            ..Default::default()
        };
    }

    pub fn set_channel(&mut self, ch: Channel, state: ChannelState) {
        match ch {
            Channel::Red    => self.red    = state,
            Channel::Yellow => self.yellow = state,
            Channel::Green  => self.green  = state,
            Channel::Buzzer => self.buzzer = state,
        }
        self.last_updated = Some(Utc::now());
    }

    pub fn get_channel(&self, ch: Channel) -> &ChannelState {
        match ch {
            Channel::Red    => &self.red,
            Channel::Yellow => &self.yellow,
            Channel::Green  => &self.green,
            Channel::Buzzer => &self.buzzer,
        }
    }
}

// ── Channel enum ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    Red,
    Yellow,
    Green,
    Buzzer,
}

impl Channel {
    pub fn all() -> [Channel; 4] {
        [Channel::Red, Channel::Yellow, Channel::Green, Channel::Buzzer]
    }
}

impl std::fmt::Display for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Channel::Red    => write!(f, "red"),
            Channel::Yellow => write!(f, "yellow"),
            Channel::Green  => write!(f, "green"),
            Channel::Buzzer => write!(f, "buzzer"),
        }
    }
}
