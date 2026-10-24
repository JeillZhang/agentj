//! Cross-module structural pins: moved-out symbols must stay out of the root
//! file *and* out of the sibling root modules.

use super::fn_needle;

/// Every symbol moved out of the root file, for the cross-module leak pin.
const MOVED_OUT_OF_ROOT: [&str; 42] = [
    "dispatch_request",
    "handle_request_from",
    "auth_gate",
    "build_progress_frame",
    "todo_items_into_wire",
    "attach_git_statuses",
    "file_change_into_wire",
    "history_events_filtered",
    "dispatch_file_list",
    "map_file_list_error",
    "dispatch_initialize",
    "dispatch_subscribe",
    "dispatch_unsubscribe",
    "dispatch_shutdown",
    "dispatch_kernel_status",
    "dispatch_permission_respond",
    "dispatch_question_respond",
    "cleanup_connection",
    "settle_owner_pending",
    "question_notifier",
    "question_settle_hook",
    "dispatch_model_list",
    "dispatch_auth_status",
    "dispatch_provider_usage",
    "dispatch_device_start",
    "dispatch_device_poll",
    "dispatch_set_api_key",
    "dispatch_clear_credential",
    "kernel_model_info",
    "dispatch_session_run",
    "record_trajectory",
    "flush_thinking",
    "is_visually_blank",
    "build_run_reply",
    "dispatch_cancel_request",
    "dispatch_session_new",
    "resolve_project_root",
    "dispatch_session_list",
    "dispatch_session_history",
    "dispatch_session_status",
    "dispatch_session_archive",
    "dispatch_session_delete",
];

/// The per-area pins only proved a moved symbol was gone from `../lib.rs`, so a
/// symbol re-added to one of the other root-level modules still passed. Pin the
/// whole table against the root modules that must stay clear of it: `web.rs`,
/// `tcp.rs` and `ws.rs` (transports and the HTTP surface). `lib.rs` itself is
/// covered by the per-area tests.
///
/// The destination modules (`dispatch.rs`, `handlers/*`, `frames.rs`,
/// `stdio.rs`, `rpc.rs`, `registry.rs`, `tests/*`) are deliberately NOT scanned:
/// they are where these symbols are *supposed* to live, so scanning them would
/// assert the opposite of the intent. A symbol leaking into `rpc.rs` or
/// `registry.rs` is still compiled and reachable, it just lives somewhere
/// unexpected, which the `fn-length` / module-size budgets surface instead.
/// Needles are built at runtime, so this assertion's own text cannot satisfy it.
#[test]
fn moved_symbols_absent_from_sibling_root_modules() {
    let files = [
        ("web.rs", include_str!("../web.rs")),
        ("tcp.rs", include_str!("../tcp.rs")),
        ("ws.rs", include_str!("../ws.rs")),
    ];
    for name in MOVED_OUT_OF_ROOT {
        let needle = fn_needle(name);
        for (file, src) in files {
            assert!(!src.contains(&needle), "{needle} leaked into {file}");
        }
    }
    let struct_needle = format!("struct {} {{", "StdioConnection");
    for (file, src) in files {
        assert!(
            !src.contains(&struct_needle),
            "{struct_needle} leaked into {file}"
        );
    }
}
