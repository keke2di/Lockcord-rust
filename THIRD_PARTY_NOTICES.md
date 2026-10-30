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
