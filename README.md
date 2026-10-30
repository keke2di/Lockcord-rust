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
- **People**: profiles (click a name or avatar), friends and requests, adding a friend by
  username, your own status (online, idle, do not disturb, invisible), new group DMs.
- **Encrypted DMs** with other LockCord users: press **Encrypt** in a DM's header. Your normal DM
  becomes end-to-end encrypted: Discord stores only ciphertext, only your two LockCords can read
  it, and the messages stay readable later. Compare the safety code once; if their key ever
  changes, LockCord stops sending until you check it.
- **Secret chats**: the lock in a DM's header. Live, peer to peer and end-to-end encrypted
  (ecdh.chat's protocol); they open only after both of you compare a 6-digit code, and nothing
  but the invite goes through Discord.
- **Purge and Guard**: delete your own messages by channel and filter, or guard a channel with
  a trigger word or a time limit. Paced, so Discord sees no burst.
- **Translation** per chat, both ways (through Google; never for secret chats or encrypted DMs).
- **Themes**: presets, every colour, fonts, layout, glass and animated backgrounds, custom CSS,
  shareable theme files. Ctrl+Shift+0 switches to the default theme if one breaks the window.
- **On the desktop**: notifications (by default who it's from, not what they said), a tray icon,
  closing to the tray, starting with Windows, and "Save media" for a message's files (programs
  get ".download" added, and every file is marked as from the internet).
- **Captchas**: when Discord asks for one (a first message to someone new, a friend request), it
  shows in a small window of its own and the message goes through once it's done.

Privacy: no analytics or crash reports go anywhere, images from Discord are fetched by LockCord
itself, the login happens on Discord's own page in a separate window (LockCord never sees your
password), and the session is kept in Windows Credential Manager.

## Building it yourself

You need Windows 10 or 11 (WebView2 comes with them) and Rust.

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

## Running

- Sign in with Discord's own login page (a separate window opens), or with a session token.
- `lockcord.exe --profile=alt` runs a second account next to the first.
- `lockcord.exe --safe-mode` starts with the default theme.
- Settings, themes and encrypted-DM contacts are kept per profile in
  `%APPDATA%\chat.lockcord.desktop`; the session and keys in Windows Credential Manager.

## License

GPL-3.0 (LICENSE). Parts are adapted from other projects; their notices are in
THIRD_PARTY_NOTICES.md.
