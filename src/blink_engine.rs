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
use crate::state::{PhysicalChannel, ChannelState, LightState};

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
    // 4 physical channels + 1 slot for the virtual yellow task
    tasks: Mutex<[Option<ChannelTask>; 5]>,
}

const YELLOW_TASK_IDX: usize = 4;

fn channel_index(ch: PhysicalChannel) -> usize {
    match ch {
        PhysicalChannel::Red    => 0,
        PhysicalChannel::Orange => 1,
        PhysicalChannel::Green  => 2,
        PhysicalChannel::Buzzer => 3,
    }
}

fn hw_on_off(ch: PhysicalChannel) -> (u8, u8) {
    match ch {
        PhysicalChannel::Red    => (HW_RED_ON,    HW_RED_OFF),
        PhysicalChannel::Orange => (HW_ORANGE_ON, HW_ORANGE_OFF),
        PhysicalChannel::Green  => (HW_GREEN_ON,  HW_GREEN_OFF),
        PhysicalChannel::Buzzer => (HW_BUZZER_ON, HW_BUZZER_OFF),
    }
}

async fn try_send(hw: &Arc<Mutex<Option<TowerHardware>>>, cmd: u8) {
    let mut lock = hw.lock().await;
    if let Some(dev) = lock.as_mut() {
        if let Err(e) = dev.send(cmd) {
            // Drop the dead handle so the reconnect monitor reopens it and
            // replays state; this blink task keeps looping and resumes sending
            // once the handle is published again.
            warn!("blink engine hw error: {e}; dropping connection for reconnect");
            *lock = None;
        }
    }
}

impl BlinkEngine {
    pub fn new(hw: Arc<Mutex<Option<TowerHardware>>>, light: Arc<Mutex<LightState>>) -> Self {
        Self {
            hw,
            light,
            tasks: Mutex::new([None, None, None, None, None]),
        }
    }

    /// Cancel any running blink task for a physical channel.
    pub async fn cancel(&self, ch: PhysicalChannel) {
        let mut tasks = self.tasks.lock().await;
        tasks[channel_index(ch)] = None;
    }

    /// Cancel the virtual yellow blink task.
    pub async fn cancel_yellow(&self) {
        let mut tasks = self.tasks.lock().await;
        tasks[YELLOW_TASK_IDX] = None;
    }

    /// Cancel all tasks (physical + yellow).
    pub async fn cancel_all(&self) {
        let mut tasks = self.tasks.lock().await;
        for t in tasks.iter_mut() {
            *t = None;
        }
    }

    // ── Physical channel operations ───────────────────────────────────────────

    pub async fn start_sw_blink(&self, ch: PhysicalChannel, on_ms: u64, off_ms: u64) {
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

    pub async fn start_pulse(&self, ch: PhysicalChannel, on_ms: u64, off_ms: u64, count: u32) {
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

    pub async fn start_timed(&self, ch: PhysicalChannel, duration_ms: u64) {
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

    pub async fn start_sequence(&self, ch: PhysicalChannel, steps: Vec<(u64, u64)>) {
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

    // ── Virtual yellow operations (red + orange + green in sync) ──────────────

    pub async fn start_yellow_sw_blink(&self, on_ms: u64, off_ms: u64) {
        // Cancel any individual tasks on the three channels first.
        {
            let mut tasks = self.tasks.lock().await;
            tasks[channel_index(PhysicalChannel::Red)]    = None;
            tasks[channel_index(PhysicalChannel::Orange)] = None;
            tasks[channel_index(PhysicalChannel::Green)]  = None;
        }

        let hw    = Arc::clone(&self.hw);
        let light = Arc::clone(&self.light);

        let handle = tokio::spawn(async move {
            loop {
                for cmd in [HW_RED_ON, HW_ORANGE_ON, HW_GREEN_ON] {
                    try_send(&hw, cmd).await;
                }
                sleep(Duration::from_millis(on_ms)).await;
                for cmd in [HW_RED_OFF, HW_ORANGE_OFF, HW_GREEN_OFF] {
                    try_send(&hw, cmd).await;
                }
                sleep(Duration::from_millis(off_ms)).await;
            }
        });

        let mut tasks = self.tasks.lock().await;
        tasks[YELLOW_TASK_IDX] = Some(ChannelTask { handle });
        let mut l = light.lock().await;
        l.set_yellow(ChannelState::SwBlink { on_ms, off_ms }, true);
    }

    pub async fn start_yellow_pulse(&self, on_ms: u64, off_ms: u64, count: u32) {
        {
            let mut tasks = self.tasks.lock().await;
            tasks[channel_index(PhysicalChannel::Red)]    = None;
            tasks[channel_index(PhysicalChannel::Orange)] = None;
            tasks[channel_index(PhysicalChannel::Green)]  = None;
        }

        let hw          = Arc::clone(&self.hw);
        let light       = Arc::clone(&self.light);
        let light_outer = Arc::clone(&self.light);

        let handle = tokio::spawn(async move {
            for i in 0..count {
                debug!("yellow pulse {}/{count}", i + 1);
                for cmd in [HW_RED_ON, HW_ORANGE_ON, HW_GREEN_ON] {
                    try_send(&hw, cmd).await;
                }
                sleep(Duration::from_millis(on_ms)).await;
                for cmd in [HW_RED_OFF, HW_ORANGE_OFF, HW_GREEN_OFF] {
                    try_send(&hw, cmd).await;
                }
                if i < count - 1 {
                    sleep(Duration::from_millis(off_ms)).await;
                }
            }
            let mut l = light.lock().await;
            l.set_yellow(ChannelState::Off, false);
        });

        let mut tasks = self.tasks.lock().await;
        tasks[YELLOW_TASK_IDX] = Some(ChannelTask { handle });
        let mut l = light_outer.lock().await;
        l.set_yellow(ChannelState::Pulse { on_ms, off_ms, remaining: count }, true);
    }

    pub async fn start_yellow_timed(&self, duration_ms: u64) {
        {
            let mut tasks = self.tasks.lock().await;
            tasks[channel_index(PhysicalChannel::Red)]    = None;
            tasks[channel_index(PhysicalChannel::Orange)] = None;
            tasks[channel_index(PhysicalChannel::Green)]  = None;
        }

        let hw          = Arc::clone(&self.hw);
        let light       = Arc::clone(&self.light);
        let light_outer = Arc::clone(&self.light);

        let handle = tokio::spawn(async move {
            for cmd in [HW_RED_ON, HW_ORANGE_ON, HW_GREEN_ON] {
                try_send(&hw, cmd).await;
            }
            sleep(Duration::from_millis(duration_ms)).await;
            for cmd in [HW_RED_OFF, HW_ORANGE_OFF, HW_GREEN_OFF] {
                try_send(&hw, cmd).await;
            }
            let mut l = light.lock().await;
            l.set_yellow(ChannelState::Off, false);
        });

        let mut tasks = self.tasks.lock().await;
        tasks[YELLOW_TASK_IDX] = Some(ChannelTask { handle });
        let mut l = light_outer.lock().await;
        l.set_yellow(ChannelState::On, true);
    }

    pub async fn start_yellow_sequence(&self, steps: Vec<(u64, u64)>) {
        {
            let mut tasks = self.tasks.lock().await;
            tasks[channel_index(PhysicalChannel::Red)]    = None;
            tasks[channel_index(PhysicalChannel::Orange)] = None;
            tasks[channel_index(PhysicalChannel::Green)]  = None;
        }

        let hw          = Arc::clone(&self.hw);
        let light       = Arc::clone(&self.light);
        let light_outer = Arc::clone(&self.light);

        let handle = tokio::spawn(async move {
            for (on_ms, off_ms) in steps {
                for cmd in [HW_RED_ON, HW_ORANGE_ON, HW_GREEN_ON] {
                    try_send(&hw, cmd).await;
                }
                sleep(Duration::from_millis(on_ms)).await;
                for cmd in [HW_RED_OFF, HW_ORANGE_OFF, HW_GREEN_OFF] {
                    try_send(&hw, cmd).await;
                }
                if off_ms > 0 {
                    sleep(Duration::from_millis(off_ms)).await;
                }
            }
            let mut l = light.lock().await;
            l.set_yellow(ChannelState::Off, false);
        });

        let mut tasks = self.tasks.lock().await;
        tasks[YELLOW_TASK_IDX] = Some(ChannelTask { handle });
        let mut l = light_outer.lock().await;
        l.set_yellow(ChannelState::On, true);
    }
}
