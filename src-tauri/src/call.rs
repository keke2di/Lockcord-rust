//! Calls: voice, camera and screen sharing in DMs, group DMs and server voice channels.
//!
//!   signalling  over the main gateway, as Discord's own client does: op 4 joins, moves, mutes
//!               and leaves (voice state), op 13 asks for a DM's call, op 18/19/20 start, stop and
//!               watch a Go Live stream. VOICE_STATE_UPDATE + VOICE_SERVER_UPDATE give the voice
//!               session, token and endpoint; CALL_* tell about DM calls and who is being rung;
//!               STREAM_* give a stream's own voice server. Ringing is REST (/call/ring).
//!   media       Serein's voice crate (voice/): its own voice WebSocket, UDP, Opus, H.264 and
//!               Discord's required end-to-end encryption (DAVE). Microphone, speakers, camera and
//!               screen capture are native, opened only during a call, never in the webview.
//!   video       decoded pictures wait here (Frames) for the window to pull them, only for the
//!               tiles it shows (call_frame), so nothing is drawn or copied while it's hidden.
//!
//! Nothing is recorded or kept: voice keys are ephemeral and live only for the call.

use client_core::screen::{Settings as ShareSettings, SourceId};
use client_core::voice::{Secret, VoiceConnection};
use discord_voice::{Controls, RemoteFrame, Status, VideoSink};
use model::Id;
use model::voice_settings::{NoiseSuppression, Processing};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::watch;

const MAX_USERS: usize = 4096;
const MAX_ROOM: usize = 99;

/// What the window may know of someone's voice state.
pub fn state_view(s: &Value) -> Value {
    json!({
        "user_id": s["user_id"], "channel_id": s["channel_id"], "guild_id": s["guild_id"],
        "self_mute": s["self_mute"].as_bool().unwrap_or(false), "self_deaf": s["self_deaf"].as_bool().unwrap_or(false),
        "mute": s["mute"].as_bool().unwrap_or(false), "deaf": s["deaf"].as_bool().unwrap_or(false),
        "self_video": s["self_video"].as_bool().unwrap_or(false), "self_stream": s["self_stream"].as_bool().unwrap_or(false),
        "suppress": s["suppress"].as_bool().unwrap_or(false),
    })
}

fn id_of(s: &str) -> Option<Id> {
    s.parse().ok()
}

// --- Settings ---------------------------------------------------------------------------------

/// The "voice" part of settings.json, as settings.rs cleaned it.
#[derive(Clone)]
pub struct VoiceSettings {
    pub input: Option<String>,
    pub output: Option<String>,
    pub camera: Option<String>,
    pub input_gain: u16,
    pub output_gain: u16,
    pub push_to_talk: bool,
    pub processing: Processing,
    pub volumes: Vec<(u64, u16)>,
}

impl VoiceSettings {
    pub fn from(v: &Value) -> VoiceSettings {
        let device = |k: &str| v[k].as_str().filter(|s| !s.is_empty()).map(str::to_owned);
        let pct = |k: &str| v[k].as_u64().map(|p| p.min(200) as u16).unwrap_or(100);
        let suppression = match v["noise"].as_str() {
            Some("off") => NoiseSuppression::Off,
            Some("webrtc") => NoiseSuppression::WebRtc,
            _ => NoiseSuppression::RnNoise,
        };
        let processing = Processing {
            suppression,
            suppression_level: 2,
            echo_cancellation: v["echo"].as_bool().unwrap_or(true),
            automatic_gain: v["agc"].as_bool().unwrap_or(true),
            sensitivity_db: if v["sensitivity"].is_null() { None } else { Some(v["sensitivity"].as_i64().unwrap_or(-55).clamp(-80, 0) as i16) },
        };
        let volumes = v["volumes"].as_object().map(|m| m.iter().filter_map(|(id, p)| Some((id.parse::<u64>().ok()?, p.as_u64()?.min(200) as u16))).take(64).collect()).unwrap_or_default();
        VoiceSettings {
            input: device("input"),
            output: device("output"),
            camera: device("camera"),
            input_gain: pct("inputGain"),
            output_gain: pct("outputGain"),
            push_to_talk: v["mode"] == "ptt",
            processing,
            volumes,
        }
    }

    fn user_volumes(&self, session: &HashMap<u64, u16>) -> [(u64, u16); 64] {
        let mut out = [(0u64, 100u16); 64];
        let mut merged: Vec<(u64, u16)> = self.volumes.clone();
        for (id, p) in session {
            merged.retain(|(i, _)| i != id);
            merged.push((*id, *p));
        }
        for (slot, v) in out.iter_mut().zip(merged.into_iter().rev().take(64)) {
            *slot = v;
        }
        out
    }
}

// --- Video frames for the window -----------------------------------------------------------------

/// One decoded picture waiting for the window. `rgb`: packed RGB (your camera's preview) rather
/// than `picture.layout`.
struct Tile {
    id: u32,
    picture: platform::video::Picture,
    rgb: bool,
}

/// Newest picture per tile: a user's camera ("<user id>"), a watched stream ("stream:<user id>")
/// or your own camera ("me").
#[derive(Default)]
pub struct Frames {
    tiles: Mutex<HashMap<String, Tile>>,
}

const RGB: u8 = 3;

impl Frames {
    fn put(&self, key: String, picture: platform::video::Picture, rgb: bool) {
        let Ok(mut tiles) = self.tiles.lock() else { return };
        let id = tiles.get(&key).map_or(1, |t| t.id.wrapping_add(1).max(1));
        if tiles.len() >= 32 && !tiles.contains_key(&key) {
            return;
        }
        tiles.insert(key, Tile { id, picture, rgb });
    }

    fn put_remote(&self, key: String, frame: &platform::video::Picture<&[u8]>) {
        if frame.is_valid() {
            self.put(key, frame.with_data(frame.data.to_vec()), false);
        }
    }

    pub fn clear(&self) {
        if let Ok(mut tiles) = self.tiles.lock() {
            tiles.clear();
        }
    }

    pub fn drop_key(&self, key: &str) {
        if let Ok(mut tiles) = self.tiles.lock() {
            tiles.remove(key);
        }
    }

    /// The tile's picture if it's newer than `since`, for a canvas about `max_width` wide:
    /// a 16-byte header (frame number u32, width u16, height u16, layout, matrix, full range,
    /// quarter turns, 4 spare) and the planes, halved while that still covers the canvas.
    pub fn take(&self, key: &str, since: u32, max_width: u32) -> Vec<u8> {
        let Ok(tiles) = self.tiles.lock() else { return Vec::new() };
        let Some(tile) = tiles.get(key).filter(|t| t.id != since) else { return Vec::new() };
        let p = &tile.picture;
        let (layout, matrix) = (
            match p.layout {
                platform::video::Layout::Rgba => 0u8,
                platform::video::Layout::Nv12 => 1,
                platform::video::Layout::I420 => 2,
            },
            match p.color.matrix {
                platform::video::Matrix::Bt601 => 0u8,
                platform::video::Matrix::Bt709 => 1,
                platform::video::Matrix::Bt2020 => 2,
                platform::video::Matrix::Smpte240m => 3,
            },
        );
        let layout = if tile.rgb { RGB } else { layout };
        let halve = max_width > 0 && p.width >= max_width * 2 && p.width % 4 == 0 && p.height % 4 == 0 && matches!(layout, 1 | 2);
        let (data, w, h) = if halve { (halved(p, layout), p.width / 2, p.height / 2) } else { (p.data.clone(), p.width, p.height) };
        let mut out = Vec::with_capacity(16 + data.len());
        out.extend_from_slice(&tile.id.to_le_bytes());
        out.extend_from_slice(&(w as u16).to_le_bytes());
        out.extend_from_slice(&(h as u16).to_le_bytes());
        out.extend_from_slice(&[layout, matrix, u8::from(p.color.full_range), (p.rotation / 90) as u8, 0, 0, 0, 0]);
        out.extend_from_slice(&data);
        out
    }
}

// Every other row and column of each plane (NV12's interleaved chroma pairs kept together).
fn halved(p: &platform::video::Picture, layout: u8) -> Vec<u8> {
    let (w, h) = (p.width as usize, p.height as usize);
    let mut out = Vec::with_capacity(p.data.len() / 4);
    let luma = &p.data[..w * h];
    for row in luma.chunks_exact(w).step_by(2) {
        out.extend(row.iter().step_by(2));
    }
    let (cw, ch) = (w / 2, h / 2);
    if layout == 1 {
        let uv = &p.data[w * h..];
        for row in uv.chunks_exact(cw * 2).take(ch).step_by(2) {
            for pair in row.chunks_exact(2).step_by(2) {
                out.extend_from_slice(pair);
            }
        }
    } else {
        for plane in p.data[w * h..].chunks_exact(cw * ch).take(2) {
            for row in plane.chunks_exact(cw).step_by(2) {
                out.extend(row.iter().step_by(2));
            }
        }
    }
    out
}

// --- The call ---------------------------------------------------------------------------------

/// The media of a call this device is in.
pub struct Media {
    audio: discord_voice::audio::Audio,
    controls: watch::Sender<Controls>,
    camera_frames: SyncSender<discord_voice::camera_video::Frame>,
    camera: Option<discord_voice::camera::Camera>,
    camera_generation: u64,
    clock: Instant,
    pub identity: Arc<discord_voice::Identity>,
    task: tauri::async_runtime::JoinHandle<()>,
    stream_audio: SyncSender<discord_voice::Frame>,
}

impl Drop for Media {
    fn drop(&mut self) {
        self.audio.set_ready(false);
        self.task.abort();
        if let Some(c) = &self.camera {
            c.stop();
        }
    }
}

/// Your own Go Live stream, or one you watch.
pub struct Stream {
    pub key: String,
    pub request: u64,
    /// (rtc server, rtc channel) from STREAM_CREATE.
    pub rtc: Option<(Id, Id)>,
    pub server: Option<(String, String)>,
    pub task: Option<tauri::async_runtime::JoinHandle<()>>,
    pub worker: Option<discord_voice::screen::Worker>,
    pub settings: Option<ShareSettings>,
    /// Watching: whose.
    pub streamer: Option<String>,
    pub phase: &'static str,
}

impl Drop for Stream {
    fn drop(&mut self) {
        if let Some(t) = &self.task {
            t.abort();
        }
    }
}

pub struct Active {
    pub channel: String,
    pub guild: Option<String>,
    /// The other person in a 1:1 DM.
    pub peer: Option<String>,
    pub request: u64,
    /// Ring the DM once the voice server is ready (the one who starts a call does).
    pub ring: bool,
    pub session: Option<String>,
    pub server: Option<(String, String)>,
    pub phase: &'static str,
    pub privacy: Option<String>,
    pub muted: bool,
    pub deafened: bool,
    pub camera: bool,
    pub camera_available: bool,
    pub ptt_held: bool,
    pub error: Option<String>,
    pub media: Option<Media>,
    pub share: Option<Stream>,
    pub watch: Option<Stream>,
    /// Volumes set during this call (0-200%), by user.
    pub volumes: HashMap<u64, u16>,
}

#[derive(Default)]
pub struct Calls {
    /// Voice states by channel, then user: who is where (server voice channels and DM calls).
    pub rooms: HashMap<String, HashMap<String, Value>>,
    /// DM calls Discord told about (CALL_CREATE/UPDATE), by channel: { ringing: [user ids] }.
    pub dm_calls: HashMap<String, Value>,
    /// People seen in voice states (their member records carry them).
    pub users: HashMap<String, Value>,
    pub active: Option<Active>,
    next_request: u64,
}

/// A frame for the gateway to send.
pub fn voice_state(guild: Option<&str>, channel: Option<&str>, muted: bool, deafened: bool, video: bool) -> Value {
    json!({ "op": 4, "d": { "guild_id": guild, "channel_id": channel, "self_mute": muted, "self_deaf": deafened, "self_video": video } })
}

impl Calls {
    pub fn request(&mut self) -> u64 {
        self.next_request += 1;
        self.next_request
    }

    /// A fresh session (READY): Discord dropped every voice state of the old one.
    pub fn reset(&mut self) {
        self.rooms.clear();
        self.dm_calls.clear();
        self.active = None;
    }

    /// A server's voice states, from READY or GUILD_CREATE.
    pub fn load_guild(&mut self, guild: &str, states: &Value) {
        self.rooms.retain(|_, room| !room.values().any(|s| s["guild_id"] == guild));
        for s in states.as_array().into_iter().flatten() {
            let mut s = s.clone();
            s["guild_id"] = json!(guild);
            self.apply_state(&s);
        }
    }

    /// One voice state: moved into its channel (or out of every channel). Returns the channels
    /// that changed.
    pub fn apply_state(&mut self, s: &Value) -> Vec<String> {
        let Some(user) = s["user_id"].as_str().filter(|u| crate::is_id(u)).map(str::to_owned) else { return Vec::new() };
        if let Some(u) = s["member"]["user"].as_object() {
            if self.users.len() < MAX_USERS || self.users.contains_key(&user) {
                self.users.insert(user.clone(), crate::user_view(&Value::Object(u.clone())));
            }
        }
        let guild = s["guild_id"].as_str();
        let mut changed = Vec::new();
        // Out of wherever they were in the same scope (a server, or the DM calls).
        for (channel, room) in self.rooms.iter_mut() {
            if room.get(&user).is_some_and(|old| old["guild_id"].as_str() == guild) && room.remove(&user).is_some() {
                changed.push(channel.clone());
            }
        }
        self.rooms.retain(|_, room| !room.is_empty());
        if let Some(channel) = s["channel_id"].as_str().filter(|c| crate::is_id(c)) {
            let room = self.rooms.entry(channel.to_owned()).or_default();
            if room.len() < MAX_ROOM || room.contains_key(&user) {
                room.insert(user, state_view(s));
            }
            if !changed.iter().any(|c| c == channel) {
                changed.push(channel.to_owned());
            }
        }
        changed
    }

    pub fn room_view(&self, channel: &str) -> Value {
        let states: Vec<Value> = self.rooms.get(channel).map(|r| r.values().cloned().collect()).unwrap_or_default();
        let users: Vec<Value> = states.iter().filter_map(|s| self.users.get(s["user_id"].as_str()?).cloned()).collect();
        json!({ "channel": channel, "states": states, "users": users })
    }

    /// Everything the window needs at start: every room and DM call.
    pub fn view(&self) -> Value {
        let rooms: serde_json::Map<String, Value> = self.rooms.keys().map(|c| (c.clone(), self.room_view(c))).collect();
        json!({ "rooms": rooms, "dmCalls": self.dm_calls, "active": self.active_view() })
    }

    pub fn active_view(&self) -> Value {
        let Some(a) = &self.active else { return Value::Null };
        json!({
            "channel": a.channel, "guild": a.guild, "phase": a.phase, "privacy": a.privacy, "error": a.error,
            "muted": a.muted, "deafened": a.deafened, "camera": a.camera, "cameraAvailable": a.camera_available,
            "sharing": a.share.as_ref().map(|s| json!({ "phase": s.phase })),
            "watching": a.watch.as_ref().map(|w| json!({ "user": w.streamer, "phase": w.phase })),
            "volumes": a.volumes.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<serde_json::Map<_, _>>(),
        })
    }

    /// CALL_CREATE / CALL_UPDATE / CALL_DELETE. Returns whether this account is being rung.
    pub fn apply_dm_call(&mut self, name: &str, data: &Value, me: &str) -> bool {
        let Some(channel) = data["channel_id"].as_str().filter(|c| crate::is_id(c)) else { return false };
        if name == "CALL_DELETE" {
            self.dm_calls.remove(channel);
            return false;
        }
        let ringing: Vec<Value> = data["ringing"].as_array().map(|r| r.iter().filter(|u| u.as_str().is_some_and(crate::is_id)).take(MAX_ROOM).cloned().collect()).unwrap_or_default();
        let rung = ringing.iter().any(|u| u == me);
        self.dm_calls.insert(channel.to_owned(), json!({ "channel": channel, "ringing": ringing, "message_id": data["message_id"] }));
        for s in data["voice_states"].as_array().into_iter().flatten() {
            self.apply_state(s);
        }
        rung
    }
}

// --- Starting the media -----------------------------------------------------------------------

/// What a status from the voice crate means for the window: the call's phase, or the privacy
/// code once the call is secured.
pub fn phase_of(status: &Status) -> Option<&'static str> {
    Some(match status {
        Status::Connecting => "connecting",
        Status::Discovering => "connecting",
        Status::Securing => "securing",
        Status::WaitingForPeer => "waiting",
        Status::Ready { .. } => "connected",
        _ => return None,
    })
}

/// Starts the call's media once both the voice session and the voice server are known. `status`
/// hears the voice crate's progress (on its thread); `ended` hears how the transport ended.
#[allow(clippy::too_many_arguments)]
pub fn start_media(
    a: &Active,
    me: &str,
    settings: &VoiceSettings,
    frames: Arc<Frames>,
    status: impl Fn(Status) -> Result<(), ()> + Send + 'static,
    audio_status: impl Fn(Result<(), &'static str>) + Send + 'static,
    ended: impl FnOnce(Result<(), &'static str>) + Send + 'static,
) -> Result<Media, String> {
    let (Some(session), Some((token, endpoint))) = (&a.session, &a.server) else { return Err("not ready".into()) };
    let credentials = VoiceConnection {
        channel: id_of(&a.channel).ok_or("bad channel")?,
        guild: a.guild.as_deref().and_then(id_of),
        user: id_of(me).ok_or("bad user")?,
        peer: a.peer.as_deref().and_then(id_of),
        session: Secret::new(session.clone()).map_err(|_| "Discord sent a bad voice session.")?,
        token: Secret::new(token.clone()).map_err(|_| "Discord sent a bad voice token.")?,
        endpoint: endpoint.clone(),
        request: a.request,
    };
    let (capture_send, capture) = sync_channel(8);
    let (playback, playback_receive) = sync_channel(8);
    let (camera_frames, camera_receive) = sync_channel(1);
    let (stream_audio, stream_audio_receive) = sync_channel(8);
    let devices = discord_voice::audio::Devices { input: settings.input.clone(), output: settings.output.clone() };
    let audio = discord_voice::audio::Audio::start(devices, capture_send, playback_receive, audio_status).map_err(str::to_owned)?;
    let muted = a.muted || settings.push_to_talk;
    audio.set_controls(muted, a.deafened);
    audio.set_processing(settings.processing);
    audio.set_gain(settings.input_gain, settings.output_gain);
    let (controls, control_receive) = watch::channel(Controls {
        muted,
        deafened: a.deafened,
        activity_threshold_db: settings.processing.sensitivity_db.unwrap_or(-70),
        camera: 0,
        user_volumes: settings.user_volumes(&a.volumes),
        stream_volume: 100,
    });
    let sink: VideoSink = Arc::new(move |frame: RemoteFrame<'_>| frames.put_remote(frame.user.to_string(), &frame.picture));
    let identity = discord_voice::Identity::generate();
    let media_identity = identity.clone();
    let task = tauri::async_runtime::spawn(async move {
        let camera = discord_voice::camera::SUPPORTED.then_some(camera_receive);
        let result = discord_voice::run_with_identity(credentials, capture, playback, control_receive, camera, Some(sink), Some(stream_audio_receive), status, media_identity).await;
        ended(result);
    });
    Ok(Media { audio, controls, camera_frames, camera: None, camera_generation: 0, clock: Instant::now(), identity, task, stream_audio })
}

impl Media {
    /// Media flows only once the call is secured (or waiting alone in an empty room).
    pub fn set_ready(&self, ready: bool) {
        self.audio.set_ready(ready);
    }

    pub fn set_controls(&self, muted: bool, deafened: bool) {
        self.audio.set_controls(muted, deafened);
        self.controls.send_modify(|c| {
            c.muted = muted;
            c.deafened = deafened;
        });
    }

    pub fn apply_settings(&self, settings: &VoiceSettings, volumes: &HashMap<u64, u16>) {
        self.audio.set_devices(discord_voice::audio::Devices { input: settings.input.clone(), output: settings.output.clone() });
        self.audio.set_processing(settings.processing);
        self.audio.set_gain(settings.input_gain, settings.output_gain);
        let threshold = settings.processing.sensitivity_db.unwrap_or(-70);
        let user_volumes = settings.user_volumes(volumes);
        self.controls.send_modify(|c| {
            c.activity_threshold_db = threshold;
            c.user_volumes = user_volumes;
        });
    }

    /// Turns the camera on: its pictures go to the call (H.264) and to your own tile (RGB).
    pub fn camera_on(&mut self, device: Option<String>, frames: Arc<Frames>, wake: Arc<dyn Fn() + Send + Sync>) -> Result<(), String> {
        if self.camera.is_some() {
            return Ok(());
        }
        self.camera_generation += 1;
        let generation = self.camera_generation;
        let send = self.camera_frames.clone();
        let start = self.clock;
        let on_frame = Arc::new(move |frame: discord_voice::camera::Frame| {
            if (1..=discord_voice::camera_video::MAX_FRAME_BYTES).contains(&frame.h264.len()) {
                let _ = send.try_send(discord_voice::camera_video::Frame { generation, timestamp: (start.elapsed().as_micros() * 90 / 1000) as u32, data: frame.h264 });
            }
            camera_frame(&frames, frame.rgb);
        });
        let camera = discord_voice::camera::Camera::start(device, on_frame, wake).map_err(str::to_owned)?;
        self.camera = Some(camera);
        self.controls.send_modify(|c| c.camera = generation);
        Ok(())
    }

    pub fn camera_off(&mut self) {
        if let Some(c) = self.camera.take() {
            c.stop();
        }
        self.controls.send_modify(|c| c.camera = 0);
    }

    pub fn camera_error(&self) -> Option<&'static str> {
        self.camera.as_ref().and_then(|c| c.error())
    }

    pub fn stream_audio(&self) -> SyncSender<discord_voice::Frame> {
        self.stream_audio.clone()
    }
}

// Your own camera picture, for your tile: packed RGB.
fn camera_frame(frames: &Frames, rgb: Vec<u8>) {
    let (w, h) = (discord_voice::camera::WIDTH as u32, discord_voice::camera::HEIGHT as u32);
    if rgb.len() != (w * h * 3) as usize {
        return;
    }
    let color = platform::video::Color { matrix: platform::video::Matrix::Bt601, full_range: true };
    frames.put("me".into(), platform::video::Picture { width: w, height: h, layout: platform::video::Layout::Rgba, color, rotation: 0, data: rgb }, true);
}

// --- Go Live ----------------------------------------------------------------------------------

/// A stream's key, as Discord names it.
pub fn stream_key(guild: Option<&str>, channel: &str, user: &str) -> String {
    match guild {
        Some(g) => format!("guild:{g}:{channel}:{user}"),
        None => format!("call:{channel}:{user}"),
    }
}

/// The screens and windows that can be shared.
pub fn share_sources() -> Result<Value, String> {
    let list = discord_voice::screen::sources().map_err(str::to_owned)?;
    Ok(json!(list.into_iter().filter_map(|s| {
        let (kind, id) = match s.id {
            SourceId::Display(id) => ("display", id),
            SourceId::Window(id) => ("window", id),
            _ => return None,
        };
        Some(json!({ "kind": kind, "id": id.to_string(), "name": s.name }))
    }).collect::<Vec<_>>()))
}

/// Share settings from the window's choice: a screen or window, 480p/720p/1080p, 15/30/60 fps.
pub fn share_settings(kind: &str, id: &str, height: u32, fps: u32, audio: bool) -> Result<ShareSettings, String> {
    let id: u64 = id.parse().map_err(|_| "bad source")?;
    let source = match kind {
        "display" => SourceId::Display(id),
        "window" => SourceId::Window(id),
        _ => return Err("bad source".into()),
    };
    let (width, height) = match height {
        480 => (854, 480),
        1080 => (1920, 1080),
        _ => (1280, 720),
    };
    let settings = ShareSettings { source, width, height, fps, cursor: true, audio };
    if !settings.valid() {
        return Err("Those share settings aren't supported.".into());
    }
    Ok(settings)
}

/// The voice connection of a stream: its own rtc server, with the call's session.
pub fn stream_credentials(s: &Stream, a: &Active, me: &str) -> Result<VoiceConnection, String> {
    let (Some((server, channel)), Some((token, endpoint)), Some(session)) = (s.rtc, &s.server, &a.session) else { return Err("not ready".into()) };
    Ok(VoiceConnection {
        channel,
        guild: Some(server),
        user: id_of(me).ok_or("bad user")?,
        peer: a.peer.as_deref().and_then(id_of),
        session: Secret::new(session.clone()).map_err(|_| "Discord sent a bad voice session.")?,
        token: Secret::new(token.clone()).map_err(|_| "Discord sent a bad stream token.")?,
        endpoint: endpoint.clone(),
        request: s.request,
    })
}

/// A sink for a watched stream's pictures, kept as "stream:<user id>".
pub fn stream_sink(frames: Arc<Frames>, streamer: String) -> VideoSink {
    let key = format!("stream:{streamer}");
    Arc::new(move |frame: RemoteFrame<'_>| frames.put_remote(key.clone(), &frame.picture))
}

/// STREAM_CREATE's connection ids.
pub fn stream_rtc(data: &Value) -> Option<(Id, Id)> {
    let server: Id = data["rtc_server_id"].as_str()?.parse().ok()?;
    let channel = data["rtc_channel_id"].as_str().and_then(|c| c.parse().ok()).unwrap_or(server);
    Some((server, channel))
}

/// Why Discord ended a stream, in words.
pub fn stream_end_reason(data: &Value) -> &'static str {
    match data["reason"].as_str() {
        Some("user_requested") => "The stream ended.",
        Some("stream_full") => "That stream is full.",
        Some("unauthorized") => "You can't watch that stream.",
        Some("safety_guild_rate_limited") => "Discord limited streaming here for now.",
        _ => "The stream ended.",
    }
}

// --- Devices ----------------------------------------------------------------------------------

/// Microphones, speakers and cameras, by id and name. Listing opens none of them.
pub fn devices() -> Value {
    let audio = discord_voice::audio::devices().ok();
    let pairs = |list: &[(String, String)]| list.iter().map(|(id, name)| json!({ "id": id, "name": name })).collect::<Vec<_>>();
    let cameras = discord_voice::camera::devices().unwrap_or_default();
    json!({
        "inputs": audio.as_ref().map(|a| pairs(&a.inputs)).unwrap_or_default(),
        "outputs": audio.as_ref().map(|a| pairs(&a.outputs)).unwrap_or_default(),
        "cameras": pairs(&cameras),
        "screens": discord_voice::screen::supported(),
    })
}

/// A microphone test: your microphone, processed as in a call, played back to you; nothing goes
/// anywhere else.
pub fn mic_test(settings: &VoiceSettings) -> Result<discord_voice::audio::Audio, String> {
    let devices = discord_voice::audio::Devices { input: settings.input.clone(), output: settings.output.clone() };
    let audio = discord_voice::audio::Audio::preview(devices, |_| {}).map_err(str::to_owned)?;
    audio.set_processing(settings.processing);
    audio.set_gain(settings.input_gain, settings.output_gain);
    audio.set_ready(true);
    Ok(audio)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_states_move_people_between_rooms() {
        let mut c = Calls::default();
        let changed = c.apply_state(&json!({ "user_id": "5", "channel_id": "100", "guild_id": "9", "self_mute": true, "member": { "user": { "id": "5", "username": "five" } } }));
        assert_eq!(changed, ["100"]);
        assert_eq!(c.room_view("100")["states"][0]["self_mute"], true);
        assert_eq!(c.room_view("100")["users"][0]["username"], "five");
        let changed = c.apply_state(&json!({ "user_id": "5", "channel_id": "101", "guild_id": "9" }));
        assert_eq!(changed, ["100", "101"]);
        assert!(c.rooms.get("100").is_none());
        // A DM call is its own scope: being in one doesn't take you out of a server's room.
        c.apply_state(&json!({ "user_id": "5", "channel_id": "300", "guild_id": null }));
        assert!(c.rooms.contains_key("101") && c.rooms.contains_key("300"));
        let changed = c.apply_state(&json!({ "user_id": "5", "channel_id": null, "guild_id": "9" }));
        assert_eq!(changed, ["101"]);
        assert!(!c.rooms.contains_key("101"));
    }

    #[test]
    fn dm_calls_say_who_is_rung() {
        let mut c = Calls::default();
        assert!(c.apply_dm_call("CALL_CREATE", &json!({ "channel_id": "300", "ringing": ["5"], "message_id": "1" }), "5"));
        assert!(!c.apply_dm_call("CALL_UPDATE", &json!({ "channel_id": "300", "ringing": [] }), "5"));
        assert!(c.dm_calls.contains_key("300"));
        c.apply_dm_call("CALL_DELETE", &json!({ "channel_id": "300" }), "5");
        assert!(c.dm_calls.is_empty());
    }

    #[test]
    fn stream_keys_and_share_settings() {
        assert_eq!(stream_key(None, "300", "5"), "call:300:5");
        assert_eq!(stream_key(Some("9"), "100", "5"), "guild:9:100:5");
        assert!(share_settings("display", "7", 720, 30, false).is_ok());
        assert!(share_settings("display", "0", 720, 30, false).is_err());
        assert!(share_settings("window", "7", 1080, 25, false).is_err());
        assert!(share_settings("portal", "7", 720, 30, false).is_err());
        assert_eq!(stream_rtc(&json!({ "rtc_server_id": "77" })), Some((Id(77), Id(77))));
    }

    #[test]
    fn frames_are_pulled_once_and_halved_for_small_tiles() {
        let f = Frames::default();
        let color = platform::video::Color { matrix: platform::video::Matrix::Bt709, full_range: false };
        let data: Vec<u8> = (0..(8 * 4 + 2 * 4 * 2)).map(|i| i as u8).collect();
        f.put("5".into(), platform::video::Picture { width: 8, height: 4, layout: platform::video::Layout::I420, color, rotation: 0, data }, false);
        let full = f.take("5", 0, 0);
        assert_eq!(&full[4..8], &[8, 0, 4, 0]);
        assert_eq!(full[8], 2);
        assert_eq!(full.len(), 16 + 48);
        assert!(f.take("5", 1, 0).is_empty(), "nothing new since frame 1");
        let small = f.take("5", 0, 4);
        assert_eq!(&small[4..8], &[4, 0, 2, 0]);
        assert_eq!(small.len(), 16 + 8 + 2 * 2);
        assert_eq!(small[16], 0, "first luma sample");
        assert_eq!(small[17], 2, "every other column");
    }

    #[test]
    fn settings_map_to_processing() {
        let s = VoiceSettings::from(&json!({ "noise": "off", "echo": false, "sensitivity": null, "inputGain": 250, "mode": "ptt", "volumes": { "5": 150 } }));
        assert_eq!(s.processing.suppression, NoiseSuppression::Off);
        assert!(!s.processing.echo_cancellation && s.processing.sensitivity_db.is_none());
        assert_eq!(s.input_gain, 200);
        assert!(s.push_to_talk);
        let mut session = HashMap::new();
        session.insert(6u64, 50u16);
        let v = s.user_volumes(&session);
        assert!(v.contains(&(5, 150)) && v.contains(&(6, 50)));
    }
}
