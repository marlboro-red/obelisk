/// Send a desktop notification (toast/banner). Fails silently if the OS
/// does not support it or the notification daemon is unavailable.
pub fn send_notification(title: &str, body: &str) {
    let _ = notify_rust::Notification::new()
        .summary(title)
        .body(body)
        .show();
}

/// Write a terminal bell character (BEL, \x07) to stderr as a lightweight
/// audio fallback for terminals that support it.
pub fn send_bell() {
    use std::io::Write;
    let _ = std::io::stderr().write_all(b"\x07");
    let _ = std::io::stderr().flush();
}
