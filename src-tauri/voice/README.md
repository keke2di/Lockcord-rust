# Calls: Serein's voice crate

This is `crates/discord-voice` from Serein (https://github.com/ViceVerse-cz/Serein, MIT OR
Apache-2.0, license files here), used by LockCord's `src/call.rs` for calls: its own voice
WebSocket and UDP transport, Opus audio, H.264 camera and screen video, Discord's DAVE end-to-end
encryption, and the microphone, speaker, camera and screen capture.

Changes for LockCord:

- Windows only: the macOS and Linux capture and encoder modules are left out.
- `support/` holds small crates with the pieces of Serein's `client-core`, `model` and `platform`
  this crate uses (the call credentials, screen-share settings, microphone-processing settings,
  output-device configuration and the live H.264 decoder), so the copied sources build unchanged.
- The test-only path to the vendored HPKE adapter points at `src-tauri/vendor/hpke-rs`.

Serein notes that its live interoperability with Discord wasn't verified when this was copied.
`cargo test -p discord-voice --lib` runs its synthetic tests (MLS/DAVE between local peers,
encrypted RTP over local sockets, Opus, jitter and loss handling); none of them touch Discord or
a real device.
