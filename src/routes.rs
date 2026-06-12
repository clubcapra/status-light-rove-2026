use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post, delete},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tracing::info;
use utoipa::{OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;

use crate::hardware::{
    HW_RED_ON,    HW_RED_OFF,    HW_RED_BLINK,
    HW_YELLOW_ON, HW_YELLOW_OFF, HW_YELLOW_BLINK,
    HW_GREEN_ON,  HW_GREEN_OFF,  HW_GREEN_BLINK,
    HW_BUZZER_ON, HW_BUZZER_OFF, HW_BUZZER_BLINK,
};
use crate::state::{Channel, ChannelState, LightState};
use crate::AppState;

// ── OpenAPI doc ───────────────────────────────────────────────────────────────

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Tower Light API",
        version = "0.1.0",
        description = "REST API for the Adafruit USB Tri-Color Tower Light with Buzzer.\n\n\
            ## Channels\n\
            - `red` — Red LED segment\n\
            - `yellow` — Yellow LED segment\n\
            - `green` — Green LED segment\n\
            - `buzzer` — Audible buzzer\n\n\
            ## Boot behavior\n\
            On startup the service sets **yellow ON** to indicate the system is initializing."
    ),
    paths(
        get_status,
        post_clear,
        post_set_all,
        post_on,
        post_off,
        delete_channel,
        post_hw_blink,
        post_sw_blink,
        post_pulse,
        post_timed,
        post_sequence,
    ),
    components(schemas(
        LightState,
        ChannelState,
        Channel,
        ApiOk,
        SetAllBody,
        BlinkBody,
        PulseBody,
        TimedBody,
        SequenceBody,
        SequenceStep,
    )),
    tags(
        (name = "status",   description = "Read current light state"),
        (name = "global",   description = "Multi-channel or reset operations"),
        (name = "channel",  description = "Per-channel control"),
        (name = "advanced", description = "Timed, pulsed, and sequenced effects"),
    )
)]
pub struct ApiDoc;

// ── Response / request schemas ────────────────────────────────────────────────

/// Standard API response
#[derive(Serialize, ToSchema)]
pub struct ApiOk {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Set multiple channels atomically. Omit a field to leave that channel unchanged.
#[derive(Deserialize, ToSchema)]
pub struct SetAllBody {
    /// Turn red LED on (true) or off (false)
    pub red:    Option<bool>,
    /// Turn yellow LED on (true) or off (false)
    pub yellow: Option<bool>,
    /// Turn green LED on (true) or off (false)
    pub green:  Option<bool>,
    /// Turn buzzer on (true) or off (false)
    pub buzzer: Option<bool>,
}

/// Software blink parameters
#[derive(Deserialize, ToSchema)]
pub struct BlinkBody {
    /// ON duration in milliseconds
    #[serde(default = "default_500")]
    #[schema(example = 500)]
    pub on_ms: u64,
    /// OFF duration in milliseconds
    #[serde(default = "default_500")]
    #[schema(example = 500)]
    pub off_ms: u64,
}

/// Pulse (blink N times then off) parameters
#[derive(Deserialize, ToSchema)]
pub struct PulseBody {
    /// Number of times to blink
    #[schema(example = 3)]
    pub count: u32,
    /// ON duration in milliseconds
    #[serde(default = "default_200")]
    #[schema(example = 200)]
    pub on_ms: u64,
    /// OFF duration in milliseconds
    #[serde(default = "default_200")]
    #[schema(example = 200)]
    pub off_ms: u64,
}

/// Timed on parameters
#[derive(Deserialize, ToSchema)]
pub struct TimedBody {
    /// How long to stay on in milliseconds, then auto-off
    #[schema(example = 2000)]
    pub duration_ms: u64,
}

/// One step in a sequence
#[derive(Deserialize, ToSchema)]
pub struct SequenceStep {
    /// ON duration in milliseconds
    #[schema(example = 200)]
    pub on_ms:  u64,
    /// OFF duration in milliseconds (0 = no pause before next step)
    #[schema(example = 100)]
    pub off_ms: u64,
}

/// Sequence of on/off steps, executed once then channel goes off
#[derive(Deserialize, ToSchema)]
pub struct SequenceBody {
    /// List of steps to execute in order
    pub steps: Vec<SequenceStep>,
}

fn default_500() -> u64 { 500 }
fn default_200() -> u64 { 200 }

// ── Helpers ───────────────────────────────────────────────────────────────────

fn ok(msg: impl Into<String>) -> (StatusCode, Json<ApiOk>) {
    (StatusCode::OK, Json(ApiOk { ok: true, message: Some(msg.into()) }))
}

fn err(status: StatusCode, msg: impl Into<String>) -> (StatusCode, Json<ApiOk>) {
    (status, Json(ApiOk { ok: false, message: Some(msg.into()) }))
}

fn parse_channel(s: &str) -> Option<Channel> {
    match s.to_lowercase().as_str() {
        "red"    => Some(Channel::Red),
        "yellow" => Some(Channel::Yellow),
        "green"  => Some(Channel::Green),
        "buzzer" => Some(Channel::Buzzer),
        _        => None,
    }
}

fn hw_on_off_blink(ch: Channel) -> (u8, u8, u8) {
    match ch {
        Channel::Red    => (HW_RED_ON,    HW_RED_OFF,    HW_RED_BLINK),
        Channel::Yellow => (HW_YELLOW_ON, HW_YELLOW_OFF, HW_YELLOW_BLINK),
        Channel::Green  => (HW_GREEN_ON,  HW_GREEN_OFF,  HW_GREEN_BLINK),
        Channel::Buzzer => (HW_BUZZER_ON, HW_BUZZER_OFF, HW_BUZZER_BLINK),
    }
}

// ── Router ────────────────────────────────────────────────────────────────────

pub fn build_router(state: AppState) -> Router {
    let api_routes = Router::new()
        .route("/status",            get(get_status))
        .route("/clear",             post(post_clear))
        .route("/set",               post(post_set_all))
        .route("/:channel/on",       post(post_on))
        .route("/:channel/off",      post(post_off))
        .route("/:channel",          delete(delete_channel))
        .route("/:channel/blink/hw", post(post_hw_blink))
        .route("/:channel/blink",    post(post_sw_blink))
        .route("/:channel/pulse",    post(post_pulse))
        .route("/:channel/timed",    post(post_timed))
        .route("/:channel/sequence", post(post_sequence))
        .with_state(state);

    Router::new()
        .merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .merge(api_routes)
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// Get current state of all channels
#[utoipa::path(
    get,
    path = "/status",
    tag = "status",
    responses(
        (status = 200, description = "Current light state", body = LightState)
    )
)]
async fn get_status(State(s): State<AppState>) -> impl IntoResponse {
    let light = s.light.lock().await;
    (StatusCode::OK, Json(light.clone()))
}

/// Turn all channels off and cancel all blink tasks
#[utoipa::path(
    post,
    path = "/clear",
    tag = "global",
    responses(
        (status = 200, description = "All channels cleared", body = ApiOk),
        (status = 500, description = "Hardware error",       body = ApiOk),
    )
)]
async fn post_clear(State(s): State<AppState>) -> impl IntoResponse {
    s.blinker.cancel_all().await;
    let mut hw = s.hw.lock().await;
    match hw.all_off() {
        Ok(_) => {
            let mut l = s.light.lock().await;
            l.clear();
            info!("ALL OFF");
            ok("All channels cleared")
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// Set multiple channels on/off in a single atomic request
#[utoipa::path(
    post,
    path = "/set",
    tag = "global",
    request_body = SetAllBody,
    responses(
        (status = 200, description = "Channels updated", body = ApiOk),
        (status = 500, description = "Hardware error",   body = ApiOk),
    )
)]
async fn post_set_all(
    State(s): State<AppState>,
    Json(body): Json<SetAllBody>,
) -> impl IntoResponse {
    let pairs: [(Option<bool>, Channel); 4] = [
        (body.red,    Channel::Red),
        (body.yellow, Channel::Yellow),
        (body.green,  Channel::Green),
        (body.buzzer, Channel::Buzzer),
    ];
    for (val, ch) in pairs {
        let Some(on) = val else { continue };
        let (on_cmd, off_cmd, _) = hw_on_off_blink(ch);
        s.blinker.cancel(ch).await;
        let mut hw = s.hw.lock().await;
        let cmd = if on { on_cmd } else { off_cmd };
        if let Err(e) = hw.send(cmd) {
            return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
        }
        let mut l = s.light.lock().await;
        l.set_channel(ch, if on { ChannelState::On } else { ChannelState::Off });
    }
    ok("Channels set")
}

/// Turn a channel on
#[utoipa::path(
    post,
    path = "/{channel}/on",
    tag = "channel",
    params(("channel" = String, Path, description = "red | yellow | green | buzzer")),
    responses(
        (status = 200, description = "Channel turned on",    body = ApiOk),
        (status = 404, description = "Unknown channel",      body = ApiOk),
        (status = 500, description = "Hardware error",       body = ApiOk),
    )
)]
async fn post_on(
    State(s): State<AppState>,
    Path(channel): Path<String>,
) -> impl IntoResponse {
    let Some(ch) = parse_channel(&channel) else {
        return err(StatusCode::NOT_FOUND, format!("Unknown channel: {channel}"));
    };
    let (on_cmd, _, _) = hw_on_off_blink(ch);
    s.blinker.cancel(ch).await;
    let mut hw = s.hw.lock().await;
    match hw.send(on_cmd) {
        Ok(_) => {
            let mut l = s.light.lock().await;
            l.set_channel(ch, ChannelState::On);
            info!("{ch} ON");
            ok(format!("{ch} on"))
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// Turn a channel off
#[utoipa::path(
    post,
    path = "/{channel}/off",
    tag = "channel",
    params(("channel" = String, Path, description = "red | yellow | green | buzzer")),
    responses(
        (status = 200, description = "Channel turned off", body = ApiOk),
        (status = 404, description = "Unknown channel",    body = ApiOk),
        (status = 500, description = "Hardware error",     body = ApiOk),
    )
)]
async fn post_off(
    State(s): State<AppState>,
    Path(channel): Path<String>,
) -> impl IntoResponse {
    channel_off(s, channel).await
}

/// Turn a channel off (DELETE alias)
#[utoipa::path(
    delete,
    path = "/{channel}",
    tag = "channel",
    params(("channel" = String, Path, description = "red | yellow | green | buzzer")),
    responses(
        (status = 200, description = "Channel turned off", body = ApiOk),
        (status = 404, description = "Unknown channel",    body = ApiOk),
        (status = 500, description = "Hardware error",     body = ApiOk),
    )
)]
async fn delete_channel(
    State(s): State<AppState>,
    Path(channel): Path<String>,
) -> impl IntoResponse {
    channel_off(s, channel).await
}

async fn channel_off(s: AppState, channel: String) -> impl IntoResponse {
    let Some(ch) = parse_channel(&channel) else {
        return err(StatusCode::NOT_FOUND, format!("Unknown channel: {channel}"));
    };
    let (_, off_cmd, _) = hw_on_off_blink(ch);
    s.blinker.cancel(ch).await;
    let mut hw = s.hw.lock().await;
    match hw.send(off_cmd) {
        Ok(_) => {
            let mut l = s.light.lock().await;
            l.set_channel(ch, ChannelState::Off);
            info!("{ch} OFF");
            ok(format!("{ch} off"))
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// Hardware native blink (~1 Hz fixed). Lowest CPU overhead.
#[utoipa::path(
    post,
    path = "/{channel}/blink/hw",
    tag = "channel",
    params(("channel" = String, Path, description = "red | yellow | green | buzzer")),
    responses(
        (status = 200, description = "Hardware blink started", body = ApiOk),
        (status = 404, description = "Unknown channel",        body = ApiOk),
        (status = 500, description = "Hardware error",         body = ApiOk),
    )
)]
async fn post_hw_blink(
    State(s): State<AppState>,
    Path(channel): Path<String>,
) -> impl IntoResponse {
    let Some(ch) = parse_channel(&channel) else {
        return err(StatusCode::NOT_FOUND, format!("Unknown channel: {channel}"));
    };
    let (_, _, blink_cmd) = hw_on_off_blink(ch);
    s.blinker.cancel(ch).await;
    let mut hw = s.hw.lock().await;
    match hw.send(blink_cmd) {
        Ok(_) => {
            let mut l = s.light.lock().await;
            l.set_channel(ch, ChannelState::HwBlink);
            info!("{ch} HW_BLINK");
            ok(format!("{ch} hardware blink"))
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// Software blink with custom on/off frequency. Runs indefinitely until cancelled.
#[utoipa::path(
    post,
    path = "/{channel}/blink",
    tag = "advanced",
    params(("channel" = String, Path, description = "red | yellow | green | buzzer")),
    request_body = BlinkBody,
    responses(
        (status = 200, description = "Blink started", body = ApiOk),
        (status = 404, description = "Unknown channel", body = ApiOk),
    )
)]
async fn post_sw_blink(
    State(s): State<AppState>,
    Path(channel): Path<String>,
    Json(body): Json<BlinkBody>,
) -> impl IntoResponse {
    let Some(ch) = parse_channel(&channel) else {
        return err(StatusCode::NOT_FOUND, format!("Unknown channel: {channel}"));
    };
    s.blinker.cancel(ch).await;
    s.blinker.start_sw_blink(ch, body.on_ms, body.off_ms).await;
    info!("{ch} SW_BLINK on={}ms off={}ms", body.on_ms, body.off_ms);
    ok(format!("{ch} blinking ({}ms on / {}ms off)", body.on_ms, body.off_ms))
}

/// Blink exactly N times then turn off automatically
#[utoipa::path(
    post,
    path = "/{channel}/pulse",
    tag = "advanced",
    params(("channel" = String, Path, description = "red | yellow | green | buzzer")),
    request_body = PulseBody,
    responses(
        (status = 200, description = "Pulse started", body = ApiOk),
        (status = 404, description = "Unknown channel", body = ApiOk),
    )
)]
async fn post_pulse(
    State(s): State<AppState>,
    Path(channel): Path<String>,
    Json(body): Json<PulseBody>,
) -> impl IntoResponse {
    let Some(ch) = parse_channel(&channel) else {
        return err(StatusCode::NOT_FOUND, format!("Unknown channel: {channel}"));
    };
    s.blinker.cancel(ch).await;
    s.blinker.start_pulse(ch, body.on_ms, body.off_ms, body.count).await;
    info!("{ch} PULSE {}x ({}ms/{}ms)", body.count, body.on_ms, body.off_ms);
    ok(format!("{ch} pulse {}x", body.count))
}

/// Turn on for a fixed duration then off automatically
#[utoipa::path(
    post,
    path = "/{channel}/timed",
    tag = "advanced",
    params(("channel" = String, Path, description = "red | yellow | green | buzzer")),
    request_body = TimedBody,
    responses(
        (status = 200, description = "Timed on started", body = ApiOk),
        (status = 404, description = "Unknown channel",  body = ApiOk),
    )
)]
async fn post_timed(
    State(s): State<AppState>,
    Path(channel): Path<String>,
    Json(body): Json<TimedBody>,
) -> impl IntoResponse {
    let Some(ch) = parse_channel(&channel) else {
        return err(StatusCode::NOT_FOUND, format!("Unknown channel: {channel}"));
    };
    s.blinker.cancel(ch).await;
    s.blinker.start_timed(ch, body.duration_ms).await;
    info!("{ch} TIMED {}ms", body.duration_ms);
    ok(format!("{ch} on for {}ms", body.duration_ms))
}

/// Execute a custom step pattern once then turn off. Useful for morse code, boot animations, alerts.
#[utoipa::path(
    post,
    path = "/{channel}/sequence",
    tag = "advanced",
    params(("channel" = String, Path, description = "red | yellow | green | buzzer")),
    request_body = SequenceBody,
    responses(
        (status = 200, description = "Sequence started",    body = ApiOk),
        (status = 400, description = "Empty steps list",    body = ApiOk),
        (status = 404, description = "Unknown channel",     body = ApiOk),
    )
)]
async fn post_sequence(
    State(s): State<AppState>,
    Path(channel): Path<String>,
    Json(body): Json<SequenceBody>,
) -> impl IntoResponse {
    let Some(ch) = parse_channel(&channel) else {
        return err(StatusCode::NOT_FOUND, format!("Unknown channel: {channel}"));
    };
    if body.steps.is_empty() {
        return err(StatusCode::BAD_REQUEST, "steps cannot be empty");
    }
    let steps: Vec<(u64, u64)> = body.steps.iter().map(|s| (s.on_ms, s.off_ms)).collect();
    s.blinker.cancel(ch).await;
    s.blinker.start_sequence(ch, steps).await;
    info!("{ch} SEQUENCE {} steps", body.steps.len());
    ok(format!("{ch} sequence ({} steps)", body.steps.len()))
}
