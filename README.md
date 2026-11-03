# LockCord

A Discord client of its own, built for privacy, with secret chats and encrypted DMs built in. No
Discord code runs in it: the window is LockCord's own UI (`ui/`), and its Rust core (`src-tauri/`)
talks to Discord's API and gateway itself.

> Third-party clients are against Discord's Terms of Service, and accounts can be disabled for
> using one. Use an account you can afford to lose.

## What it does

- **DMs, group DMs and servers**: messages, replies, reactions, edits, images, GIFs and videos,
  files, embeds, custom emoji, polls, pins, threads and forum posts (read and reply), the member
  list, unread markers, and search (`from:`, `has:`, `in:`, dates).
- **People**: full profiles (click a name or avatar) with status, custom status, mutual servers
  and mutual friends; friends and requests, adding a friend by username, your own status (online,
  idle, do not disturb, invisible), new group DMs.
- **Emoji and GIF picker**: every emoji, searchable, with the server's own (every server's with
  Nitro) and your recent ones; GIFs from Discord's GIF search (Tenor), trending and your favorites.
- **Calls** in DMs, group DMs and server voice channels: voice, camera, screen sharing (Go Live)
  and watching others' streams, ringing, mute, deafen, push-to-talk, per-person volume, and
  optional global mute and deafen keys. Calls are end-to-end encrypted with Discord's DAVE
  protocol; compare the call's code to check. Microphone, camera and screen open only during a
  call. Stage channels aren't supported.
- **Encrypted DMs** with other LockCord users: press **Encrypt** in a DM's header. Their LockCord
  asks them first; once they accept, your normal DM is end-to-end encrypted: Discord stores only
  ciphertext, only your two LockCords can read it, and the messages stay readable later. Compare
  the 60-digit safety code once; if their key ever changes, LockCord stops sending until you check
  it.
- **Secret chats**: the lock in a DM's header. Live, peer to peer and end-to-end encrypted
  (ecdh.chat's protocol); they open only after both of you compare a 6-digit code, and nothing
  but the invite goes through Discord.
- **Purge and Guard**: delete your own messages by channel and filter, or guard a channel with
  a trigger word or a time limit. Paced, so Discord sees no burst.
- **Translation** per chat, both ways (through Google; never for secret chats or encrypted DMs).
- **Privacy tools** (Settings → Privacy):
  - **Screen capture shield**: screenshots, recorders and screen sharing leave LockCord's window
    out while an encrypted DM or a secret chat is on screen (or always).
  - **Ghost mode**, everywhere or per chat (the ghost in a chat's header): nobody sees you
    typing, and Discord isn't told what you read.
  - **Clean photos and links**: the location (GPS), camera, time and other metadata come out of
    JPEG, PNG and WebP images before they're uploaded (a photo keeps its rotation), and
    tracking parameters (`utm_…`, `fbclid`, `gclid`, YouTube's and Spotify's `si`, …) come out of
    links you send. Optionally generic file names, and links sent without previews.
  - **Panic key**: one global shortcut hides LockCord in the tray and mutes your
    microphone.
  - **Protected clipboard**: text copied from a secret chat or an encrypted DM stays out of
    Windows' clipboard history and cloud clipboard, and is cleared after 30 seconds.
  - **What LockCord sends**: every host LockCord contacted this session, with a request count.
- **Themes**: seven complete looks (LockCord, Midnight Pro, Blossom, Windows XP, Aero Glass,
  Sakura Retro, Minimal), each changing shapes, frames, buttons, icons and animations, with its own
  light and dark colour schemes. Text size, compact messages and animations can be set on top.
  Ctrl+Shift+0 switches to the default theme for a moment.
- **On the desktop**: notifications (by default who it's from, not what they said), a tray icon,
  closing to the tray, starting with Windows, and "Save media" for a message's files (programs
  get ".download" added, and every file is marked as from the internet).
- **Captchas**: when Discord asks for one (a first message to someone new, a friend request), it
  shows in a small window of its own and the message goes through once it's done.
- **Autocomplete** in the composer: `@` for people, roles, @everyone and @here, `#` for channels,
  `:` for emoji (and the server's own). In a server, people not seen yet are searched through
  Discord's gateway as Discord's app does; in DMs nothing typed is sent for a search.
- **Updates**: once a day LockCord asks GitHub whether a newer version is out, and offers it. An
  installed LockCord downloads the installer, checks it against the checksum GitHub publishes, and
  starts it once you agree; a portable copy opens the release page. Turn it off in Settings → App.

Privacy: no analytics or crash reports go anywhere, images from Discord (and GIF previews from
Tenor) are fetched by LockCord itself, the login happens on Discord's own page in a separate window
(LockCord never sees your password), and the session is kept in Windows Credential Manager, on
this computer only (never roaming to other machines) and encrypted for your Windows account.
Calls connect to Discord's voice servers directly, as Discord's own app does, so Discord sees your
IP address in a call. The update check asks `api.github.com` (with no token or anything about
you), and can be turned off.

The protected clipboard keeps copied secrets out of clipboard history and sync. It doesn't stop
programs running as your user account, which can read what LockCord keeps. Ghost mode doesn't hide that you're
online: set your status to Invisible for that.

### What encrypted DMs don't protect

- **No forward secrecy.** The message keys come from your two LockCords' identity keys, so anyone
  who later got hold of your identity key (and Discord's copy of the ciphertext) could read that
  DM's past messages. "Turn off" in the DM forgets its keys.
- **Your own computer.** Programs running as your Windows user can read the keys LockCord keeps.
- **What Discord still sees:** that you two talk, when, roughly how long each message is,
  reactions, typing, edits and deletes, and the key messages themselves (public keys only).
  Files go unencrypted, and only on purpose.

## Installing

Download `LockCord_<version>_x64-setup.exe` from the releases and run it. It installs for your
Windows user only (no administrator rights), with a Start menu entry and an uninstaller. It isn't
signed, so Windows SmartScreen may warn: choose **More info**, then **Run anyway**. The portable
`lockcord.exe` works too, without installing.

## Building it yourself

You need Windows 10 or 11 (WebView2 comes with them), Rust, and CMake (for the Opus codec used in
calls: <https://cmake.org/download/>).

1. Install Rust from <https://rustup.rs>. If it asks for the Visual C++ build tools, install
   them (Visual Studio Build Tools, "Desktop development with C++").
2. In this folder:

   ```
   cd src-tauri
   cargo build --release
   ```

   The first build takes a few minutes. The program is then
   `src-tauri\target\release\lockcord.exe`; it's a single file you can run from anywhere.

To run straight from the sources instead: `cd src-tauri` and `cargo run --release`.

To build the installer too, use Tauri's CLI in `src-tauri`: `npx @tauri-apps/cli@2 build`. The
installer is then under `target\release\bundle\nsis`.

## Running

- Sign in with Discord's own login page (a separate window opens), or with a session token.
- `lockcord.exe --profile=alt` runs a second account next to the first.
- `lockcord.exe --safe-mode` starts with the default theme.
- Settings, themes and encrypted-DM contacts are kept per profile in
  `%APPDATA%\chat.lockcord.desktop`; the session and keys in Windows Credential Manager (on this
  computer only, encrypted for your Windows account).

## License

GPL-3.0 (LICENSE). Parts are adapted from other projects; their notices are in
THIRD_PARTY_NOTICES.md.
