use std::io::{self, Write};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

/// Copy text to the system clipboard via OSC 52.
///
/// When running inside tmux, wraps the sequence in a DCS passthrough so it
/// reaches the outer terminal. Writes directly to /dev/tty to bypass
/// ratatui's alternate screen buffer.
pub fn copy_osc52(text: &str) -> io::Result<()> {
    let encoded = BASE64.encode(text);
    let osc52 = format!("\x1b]52;c;{encoded}\x07");

    let payload = if std::env::var_os("TMUX").is_some() {
        // Bare OSC 52 so tmux stores it in its paste buffer, then DCS
        // passthrough so the parent terminal picks it up too.
        let passthrough = format!("\x1bPtmux;\x1b{osc52}\x1b\\");
        format!("{osc52}{passthrough}")
    } else {
        osc52
    };

    let mut tty = std::fs::OpenOptions::new().write(true).open("/dev/tty")?;
    tty.write_all(payload.as_bytes())?;
    tty.flush()
}
