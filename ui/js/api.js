// The UI's only way out: LockCord's own commands (src-tauri/src/main.rs) and the events it
// sends. The UI never talks to Discord itself; even Discord's images come through Rust, by the
// `lcmedia:` scheme (src-tauri/src/media.rs).

const tauri = window.__TAURI__;

export const call = (command, args) => tauri.core.invoke(command, args);
export const on = (event, handler) => tauri.event.listen(event, (e) => handler(e.payload));

// A Discord media URL as this page may load it.
export const media = (url) => (url ? tauri.core.convertFileSrc(url, 'lcmedia') : null);

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
  upload: (channelId, name, bytes, plain) => tauri.core.invoke('upload_attachment', bytes, { headers: { 'lc-channel': channelId, 'lc-name': encodeURIComponent(name), 'lc-plain': plain === true ? '1' : '0' } }),
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
  subscribe: (guildId, channelId) => call('subscribe', { guildId, channelId }),
  // Settings and themes (settings.rs): kept by Rust per profile; the window keeps nothing.
  settingsGet: () => call('settings_get'),
  settingsSave: (settings) => call('settings_save', { settings }),
  themeGet: (id) => call('theme_get', { id }),
  themeSave: (theme) => call('theme_save', { theme }),
  themeDelete: (id) => call('theme_delete', { id }),
  themeAssetAdd: (id, name, bytes) => tauri.core.invoke('theme_asset_add', bytes, { headers: { 'lc-theme': id, 'lc-name': encodeURIComponent(name) } }),
  themeAssetRemove: (id, name) => call('theme_asset_remove', { id, name }),
  // Native dialogs in Rust: the window never sees a path.
  themeImport: () => call('theme_import'),
  themeExport: (id) => call('theme_export', { id }),
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
  // The desktop (desktop.rs): which chat is on screen, saving files to a folder Rust keeps.
  viewing: (channelId) => call('viewing', { channelId: channelId || null }),
  saveMedia: (files) => call('save_media', { files }),
  saveFolder: () => call('save_folder'),
  pickSaveFolder: () => call('pick_save_folder'),
  openLink: (url) => call('open_link', { url }),
};
