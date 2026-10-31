// The UI's only way out: LockCord's own commands (src-tauri/src/main.rs) and the events it
// sends. The UI never talks to Discord itself; even Discord's images come through Rust, by the
// `lcmedia:` scheme (src-tauri/src/media.rs).

// Tauri's own bridge, without the `window.__TAURI__` global (withGlobalTauri is off, so pages in
// other windows, like Discord's login page, don't get it either).
const bridge = window.__TAURI_INTERNALS__;

export const call = (command, args, options) => bridge.invoke(command, args, options);
export const on = (event, handler) => call('plugin:event|listen', {
  event, target: { kind: 'Any' }, handler: bridge.transformCallback((e) => handler(e.payload)),
});
export const fileSrc = (path, scheme) => bridge.convertFileSrc(path, scheme);

// A Discord media URL as this page may load it.
export const media = (url) => (url ? fileSrc(url, 'lcmedia') : null);

export const api = {
  status: () => call('status'),
  signIn: () => call('sign_in'),
  signInWithToken: (token) => call('sign_in_with_token', { token }),
  signOut: () => call('sign_out'),
  // The newest 50, or 50 before / after / around a message (opts: { after } or { around }).
  messages: (channelId, before, opts) => call('messages', { channelId, before: before || null, after: (opts && opts.after) || null, around: (opts && opts.around) || null }),
  // plain: true sends this one message unencrypted in an encrypted DM; otherwise Rust seals it.
  send: (channelId, content, replyTo, ping, attachments, plain) => call('send_message', {
    channelId, content, replyTo: replyTo || null, ping: ping === false ? false : null, attachments: attachments && attachments.length ? attachments : null,
    plain: plain === true ? true : null,
  }),
  // One file's bytes, uploaded to Discord's bucket; the answer goes into send's attachments.
  upload: (channelId, name, bytes, plain) => call('upload_attachment', bytes, { headers: { 'lc-channel': channelId, 'lc-name': encodeURIComponent(name), 'lc-plain': plain === true ? '1' : '0' } }),
  edit: (channelId, messageId, content) => call('edit_message', { channelId, messageId, content }),
  deleteMessage: (channelId, messageId) => call('delete_message', { channelId, messageId }),
  react: (channelId, messageId, emoji, add) => call('react', { channelId, messageId, emoji, add }),
  typing: (channelId) => call('typing', { channelId }),
  ack: (channelId, messageId) => call('ack', { channelId, messageId }),
  // action: 'accept' (an incoming request), 'remove' (a friend, a request, a block), or from a
  // profile 'add' (send a request) and 'block'.
  relationship: (userId, action) => call('relationship', { userId, action }),
  openDm: (userId) => call('open_dm', { userId }),
  // People (people.rs): profiles, a friend request by username, this account's status, groups.
  userProfile: (userId, guildId) => call('user_profile', { userId, guildId: guildId || null }),
  addFriend: (username) => call('add_friend', { username }),
  setStatus: (status) => call('set_status', { status }),
  createGroup: (userIds) => call('create_group', { userIds }),
  groupAdd: (channelId, userId) => call('group_add', { channelId, userId }),
  groupLeave: (channelId) => call('group_leave', { channelId }),
  // Channels (main.rs, guilds.rs): pins, threads, forum posts, polls.
  pins: (channelId) => call('pins', { channelId }),
  pin: (channelId, messageId, on) => call('pin', { channelId, messageId, on }),
  threads: (channelId, before) => call('threads', { channelId, before: before || null }),
  threadJoin: (threadId, join) => call('thread_join', { threadId, join }),
  threadOpen: (threadId) => call('thread_open', { threadId }),
  forumPosts: (channelId, offset) => call('forum_posts', { channelId, offset: offset || 0 }),
  pollVote: (channelId, messageId, answers) => call('poll_vote', { channelId, messageId, answers }),
  // Search (search.rs): a server (guildId) or a DM (channelId); never an encrypted DM.
  search: (guildId, channelId, query, offset) => call('search', { guildId: guildId || null, channelId: channelId || null, query, offset: offset || 0 }),
  // Opening a server channel: Rust sends the web client's op 37 for it (typing, member list).
  subscribe: (guildId, channelId, ranges) => call('subscribe', { guildId, channelId, ranges: ranges || null }),
  gifTrending: () => call('gif_trending'),
  gifSearch: (q) => call('gif_search', { q: q || null }),
  gifFavorites: () => call('gif_favorites'),
  // Settings and themes (settings.rs): kept by Rust per profile; the window keeps nothing.
  settingsGet: () => call('settings_get'),
  settingsSave: (settings) => call('settings_save', { settings }),
  // Native dialogs in Rust: the window never sees a path.
  // Purge and Guard (deletion.rs, guard.rs): every delete goes through Rust's one paced queue.
  purgeScan: (channels, filters) => call('purge_scan', { channels, filters }),
  purgeRun: () => call('purge_run'),
  purgeStop: () => call('purge_stop'),
  guardList: () => call('guard_list'),
  guardSet: (rule) => call('guard_set', { rule }),
  guardRemove: (channelId) => call('guard_remove', { channelId }),
  // Translation (translate.rs): the text goes to Google; never for secret chats or encrypted DMs.
  translateTexts: (channelId, to, texts) => call('translate_texts', { channelId, to, texts }),
  translateOutgoing: (channelId, to, text) => call('translate_outgoing', { channelId, to, text }),
  // Encrypted DMs (e2e.rs): keys never leave Rust; these carry public keys and nonces only.
  e2eStatus: () => call('e2e_status'),
  e2eEnable: (channelId) => call('e2e_enable', { channelId }),
  e2eAcceptChange: (peerId) => call('e2e_accept_change', { peerId }),
  e2eVerify: (peerId, verified) => call('e2e_verify', { peerId, verified }),
  e2eForget: (peerId) => call('e2e_forget', { peerId }),
  // The other side's offer: answered (posts this side's key) or left alone (posts nothing).
  e2eAcceptOffer: (peerId) => call('e2e_accept_offer', { peerId }),
  e2eIgnoreOffer: (peerId) => call('e2e_ignore_offer', { peerId }),
  // Calls (call.rs): voice, camera and screen sharing; media stays in Rust.
  callJoin: (channelId) => call('call_join', { channelId }),
  callLeave: () => call('call_leave'),
  callDecline: (channelId) => call('call_decline', { channelId }),
  callPeek: (channelId) => call('call_peek', { channelId }),
  callControls: (muted, deafened) => call('call_controls', { muted, deafened }),
  callPtt: (held) => call('call_ptt', { held }),
  callCamera: (on) => call('call_camera', { on }),
  callVolume: (userId, percent) => call('call_volume', { userId, percent }),
  callShareSources: () => call('call_share_sources'),
  callShare: (kind, id, height, fps, audio) => call('call_share', { kind, id, height, fps, audio }),
  callShareStop: () => call('call_share_stop'),
  callWatch: (userId) => call('call_watch', { userId }),
  callWatchStop: () => call('call_watch_stop'),
  // A video tile's newest picture as raw bytes (empty if nothing newer than `since`).
  callFrame: (key, since, maxWidth) => call('call_frame', { key, since, maxWidth }),
  voiceDevices: () => call('voice_devices'),
  // The composer's @mention search in a server (main.rs member_search; answered in lc://members-chunk).
  memberSearch: (guildId, query, nonce) => call('member_search', { guildId, query, nonce }),
  // Updates (update.rs): force asks GitHub now; otherwise at most once a day, if it's on.
  updateCheck: (force) => call('update_check', { force: !!force }),
  updateDismiss: (version) => call('update_dismiss', { version }),
  updateInstall: () => call('update_install'),
  voiceTest: (on) => call('voice_test', { on }),
  voiceTestLevel: () => call('voice_test_level'),
  // The desktop (desktop.rs): which chat is on screen, saving files to a folder Rust keeps.
  viewing: (channelId) => call('viewing', { channelId: channelId || null }),
  saveMedia: (files) => call('save_media', { files }),
  saveFolder: () => call('save_folder'),
  pickSaveFolder: () => call('pick_save_folder'),
  openLink: (url) => call('open_link', { url }),
};
