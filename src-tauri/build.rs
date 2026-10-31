// LockCord's own commands, listed so Tauri checks them against capabilities/default.json: without
// an app manifest, any window on a local origin (the captcha page's lccaptcha: scheme counts as
// one) could call them all. Keep in step with main.rs's generate_handler! (check-ui.mjs checks).
const COMMANDS: &[&str] = &[
    "status", "sign_in", "sign_in_with_token", "sign_out", "messages", "send_message", "edit_message", "delete_message", "react", "typing",
    "upload_attachment", "ack", "relationship", "open_dm", "subscribe", "settings_get", "settings_save", "purge_scan", "purge_run", "purge_stop", "guard_list", "guard_set",
    "guard_remove", "translate_texts", "translate_outgoing", "e2e_status", "e2e_enable", "e2e_accept_change", "e2e_verify", "e2e_forget", "viewing",
    "save_folder", "pick_save_folder", "save_media", "open_link", "add_friend", "user_profile", "set_status", "create_group", "group_add",
    "group_leave", "pins", "pin", "threads", "thread_join", "forum_posts", "poll_vote", "search", "thread_open", "e2e_accept_offer",
    "e2e_ignore_offer", "call_join", "call_leave", "call_decline", "call_peek", "call_controls", "call_ptt", "call_camera", "call_volume",
    "call_share_sources", "call_share", "call_share_stop", "call_watch", "call_watch_stop", "call_frame", "voice_devices", "voice_test",
    "voice_test_level", "update_check", "update_dismiss", "update_install", "member_search", "gif_trending", "gif_search", "gif_favorites",
];

fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(COMMANDS))).expect("tauri-build");
}
