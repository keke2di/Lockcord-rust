# Third-party notices

LockCord is licensed under GPL-3.0 (LICENSE). Parts of it are adapted from the projects below,
whose notices are kept here as their licenses require.

## Serein

https://github.com/ViceVerse-cz/Serein, licensed under MIT OR Apache-2.0. Adapted:

- `src-tauri/src/fingerprint.rs`: the client identity (user agent, super-properties, nonce),
  from `crates/client-core/src/fingerprint.rs`.
- `src-tauri/src/auth.rs`: the login token handoff script, from `crates/platform/src/login-handoff.js`.
- `src-tauri/src/gateway.rs`: the size bounds on gateway payloads, from `crates/discord-gateway`.
- `src-tauri/src/guilds.rs`: the member list id (`member_list_id`, `murmur3`), from
  `crates/model/src/permissions.rs`.
- `src-tauri/src/captcha.rs`: showing Discord's hCaptcha in its own page (hCaptcha's native-app host
  mode, `setData` for rqdata), from `crates/platform/src/captcha.js`.
- `src-tauri/voice/`: Serein's voice crate (`crates/discord-voice`) for calls: the voice WebSocket,
  UDP transport, Opus audio, H.264 camera and screen video, DAVE end-to-end encryption, devices
  and capture. Copied with its Linux modules left out (the macOS ones, `screen/capture_macos.rs`,
  `video_encode_macos.rs` and `audio/permission_macos.rs`, unchanged); `voice/support/` holds the
  few pieces of Serein's `client-core`, `model` and `platform` crates it uses (unchanged files:
  `screen.rs`, `voice_settings.rs`, `audio.rs`, `picture.rs`, `live_windows.rs`, `live_apple.rs`;
  `video/apple.rs` trimmed to the VideoToolbox session and NV12 copy that live decoding uses). Its
  license files are in `src-tauri/voice/`. The macOS entitlements follow Serein's
  `packaging/macos/voice.entitlements`.
- `src-tauri/src/call.rs`: the call signalling (gateway opcodes 4, 13, 18, 19, 20, ringing, stream
  keys) follows `crates/discord-gateway/src/voice.rs` and the desktop's call wiring.
- `src-tauri/vendor/davey` and `src-tauri/vendor/hpke-rs`: dependencies as Serein patches them
  (see their `SEREIN-PATCH.md`).

Used under the MIT license:

```
MIT License

Copyright (c) 2026 Serein contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Libraries used for calls

Built into LockCord as dependencies, each under its own license (the license texts ship with
their sources):

- davey 0.1.4 (Discord's DAVE protocol), MIT, https://github.com/Snazzah/davey; vendored with
  Serein's manifest patch in `src-tauri/vendor/davey` (its LICENSE included).
- hpke-rs 0.6.1, MPL-2.0, https://github.com/cryspen/hpke-rs; vendored with Serein's security
  patch in `src-tauri/vendor/hpke-rs`. Its source, as modified, is in this repository, as the
  MPL-2.0 requires.
- OpenMLS 0.8.1, MIT.
- opus2 0.4.0 (MIT OR Apache-2.0) with libopus, BSD-3-Clause, built from source.
- openh264 0.9.8 with Cisco's OpenH264, BSD-2-Clause, built from source. H.264 may be covered by
  patents in some countries.
- cpal 0.18.2, Apache-2.0; sonora 0.2.0 (echo cancellation) and nnnoiseless 0.5.2 (noise
  suppression), BSD-3-Clause; windows-capture 2.0.1, MIT; rtrb, MIT OR Apache-2.0.

## Concord

https://github.com/chojs23/concord, "Concord - A terminal user interface client for Discord",
Copyright (C) 2025-2026 chojs23, licensed under GPL-3.0, the same license as LockCord (LICENSE).
Adapted:

- `src-tauri/src/people.rs`: editing the status in Discord's settings protobuf (field numbers,
  the read-modify-write with the data version), from `src/discord/rest/user_settings.rs`; the
  profile request, from `src/discord/rest/profile.rs`.
- `src-tauri/src/search.rs`: the message search parameters and the 202 "still indexing" wait,
  from `src/discord/rest/search.rs`.
- `src-tauri/src/guilds.rs` and `main.rs`: keeping threads (THREAD_LIST_SYNC, members), the
  archived threads and thread membership requests, and the poll vote request, from
  `src/discord/thread/`, `src/discord/rest/forum.rs` and `src/discord/rest/polls.rs`.

## ecdh.chat

`ui/secure/js/` is an unmodified copy of ecdh.chat's protocol modules (`public/js`), at the commit
named in `ui/secure/ECDH_VERSION`.

## emojibase-data

`ui/js/emoji-data.js` is generated (`local/scripts/gen-emoji.mjs`) from emojibase-data 17.0.0
(https://github.com/milesj/emojibase): the English emoji names, groups, versions, and the
iamcal and CLDR shortcodes.

MIT License

Copyright (c) 2017-2019 Miles Johnson

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
