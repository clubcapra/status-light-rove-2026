use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tracing::{debug, warn};

use crate::hardware::{
    TowerHardware,
    HW_RED_ON,    HW_RED_OFF,
    HW_ORANGE_ON, HW_ORANGE_OFF,
    HW_GREEN_ON,  HW_GREEN_OFF,
    HW_BUZZER_ON, HW_BUZZER_OFF,
};
use crate::state::{Channel, ChannelState, LightState};

// ── Per-channel blink task handle ─────────────────────────────────────────────

struct ChannelTask {
    handle: JoinHandle<()>,
}

impl Drop for ChannelTask {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

// ── BlinkEngine ───────────────────────────────────────────────────────────────

pub struct BlinkEngine {
    hw:    Arc<Mutex<Option<TowerHardware>>>,
    light: Arc<Mutex<LightState>>,
    tasks: Mutex<[Option<ChannelTask>; 4]>,
}

fn channel_index(ch: Channel) -> usize {
    match ch {
        Channel::Red    => 0,
        Channel::Orange => 1,
        Channel::Green  => 2,
        Channel::Buzzer => 3,
    }
}

fn hw_on_off(ch: Channel) -> (u8, u8) {
    match ch {
        Channel::Red    => (HW_RED_ON,    HW_RED_OFF),
        Channel::Orange => (HW_ORANGE_ON, HW_ORANGE_OFF),
        Channel::Green  => (HW_GREEN_ON,  HW_GREEN_OFF),
        Channel::Buzzer => (HW_BUZZER_ON, HW_BUZZER_OFF),
    }
}

async fn try_send(hw: &Arc<Mutex<Option<TowerHardware>>>, cmd: u8) {
    let mut lock = hw.lock().await;
    if let Some(ref mut dev) = *lock {
        if let Err(e) = dev.send(cmd) {
            warn!("blink engine hw error: {e}");
        }
    }
}

impl BlinkEngine {
    pub fn new(hw: Arc<Mutex<Option<TowerHardware>>>, light: Arc<Mutex<LightState>>) -> Self {
        Self {
            hw,
            light,
            tasks: Mutex::new([None, None, None, None]),
        }
    }

    pub async fn cancel(&self, ch: Channel) {
        let mut tasks = self.tasks.lock().await;
        tasks[channel_index(ch)] = None;
    }

    pub async fn cancel_all(&self) {
        let mut tasks = self.tasks.lock().await;
        for t in tasks.iter_mut() {
            *t = None;
        }
    }

    pub async fn start_sw_blink(&self, ch: Channel, on_ms: u64, off_ms: u64) {
        let hw    = Arc::clone(&self.hw);
        let light = Arc::clone(&self.light);
        let (on_cmd, off_cmd) = hw_on_off(ch);

        let handle = tokio::spawn(async move {
            loop {
                try_send(&hw, on_cmd).await;
                sleep(Duration::from_millis(on_ms)).await;
                try_send(&hw, off_cmd).await;
                sleep(Duration::from_millis(off_ms)).await;
            }
        });

        let mut tasks = self.tasks.lock().await;
        tasks[channel_index(ch)] = Some(ChannelTask { handle });

        let mut l = light.lock().await;
        l.set_channel(ch, ChannelState::SwBlink { on_ms, off_ms });
    }

    pub async fn start_pulse(&self, ch: Channel, on_ms: u64, off_ms: u64, count: u32) {
        let hw          = Arc::clone(&self.hw);
        let light       = Arc::clone(&self.light);
        let light_outer = Arc::clone(&self.light);
        let (on_cmd, off_cmd) = hw_on_off(ch);

        let handle = tokio::spawn(async move {
            for i in 0..count {
                debug!("pulse {ch} {}/{count}", i + 1);
                try_send(&hw, on_cmd).await;
                sleep(Duration::from_millis(on_ms)).await;
                try_send(&hw, off_cmd).await;
                if i < count - 1 {
                    sleep(Duration::from_millis(off_ms)).await;
                }
            }
            let mut l = light.lock().await;
            l.set_channel(ch, ChannelState::Off);
        });

        let mut tasks = self.tasks.lock().await;
        tasks[channel_index(ch)] = Some(ChannelTask { handle });

        let mut l = light_outer.lock().await;
        l.set_channel(ch, ChannelState::Pulse { on_ms, off_ms, remaining: count });
    }

    pub async fn start_timed(&self, ch: Channel, duration_ms: u64) {
        let hw          = Arc::clone(&self.hw);
        let light       = Arc::clone(&self.light);
        let light_outer = Arc::clone(&self.light);
        let (on_cmd, off_cmd) = hw_on_off(ch);

        let handle = tokio::spawn(async move {
            try_send(&hw, on_cmd).await;
            sleep(Duration::from_millis(duration_ms)).await;
            try_send(&hw, off_cmd).await;
            let mut l = light.lock().await;
            l.set_channel(ch, ChannelState::Off);
        });

        let mut tasks = self.tasks.lock().await;
        tasks[channel_index(ch)] = Some(ChannelTask { handle });

        let mut l = light_outer.lock().await;
        l.set_channel(ch, ChannelState::On);
    }

    pub async fn start_sequence(&self, ch: Channel, steps: Vec<(u64, u64)>) {
        let hw          = Arc::clone(&self.hw);
        let light       = Arc::clone(&self.light);
        let light_outer = Arc::clone(&self.light);
        let (on_cmd, off_cmd) = hw_on_off(ch);

        let handle = tokio::spawn(async move {
            for (on_ms, off_ms) in steps {
                try_send(&hw, on_cmd).await;
                sleep(Duration::from_millis(on_ms)).await;
                try_send(&hw, off_cmd).await;
                if off_ms > 0 {
                    sleep(Duration::from_millis(off_ms)).await;
                }
            }
            let mut l = light.lock().await;
            l.set_channel(ch, ChannelState::Off);
        });

        let mut tasks = self.tasks.lock().await;
        tasks[channel_index(ch)] = Some(ChannelTask { handle });

        let mut l = light_outer.lock().await;
        l.set_channel(ch, ChannelState::On);
    }
}
