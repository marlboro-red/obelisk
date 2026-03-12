//! Integration tests for the PTY subsystem.
//!
//! Verifies portable-pty + vt100 work correctly on this platform.
//!
//! On Windows ConPTY, child.wait() and reader.read() can deadlock each other.
//! The real app avoids this by running both in async tasks. For tests, we
//! use a simpler approach: let the command run, drop the PTY handles to
//! unblock the reader, then collect output from the channel.

use portable_pty::{CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::Duration;

fn default_size() -> PtySize {
    PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// Spawn a command in a PTY, wait for it to produce output, then collect.
///
/// On Windows ConPTY, `Child::drop()` calls `wait()` which deadlocks if the
/// reader pipe is still open. We avoid this by: dropping master first (unblocks
/// reader → closes pipe → allows child cleanup), then forgetting the child
/// handle entirely to avoid the blocking destructor.
fn spawn_and_collect(cmd: CommandBuilder) -> Vec<u8> {
    let pty_system = portable_pty::native_pty_system();
    let pair = pty_system.openpty(default_size()).expect("openpty failed");

    let child = pair.slave.spawn_command(cmd).expect("spawn failed");
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().expect("clone reader failed");
    let mut writer = pair.master.take_writer().expect("take writer failed");

    // Reader thread sends output chunks through channel
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // ConPTY sends ESC[6n (Device Status Report) on startup and buffers
    // child output until it gets a cursor position response.
    std::thread::sleep(Duration::from_millis(200));
    let _ = writer.write_all(b"\x1b[1;1R");
    let _ = writer.flush();

    // Let the command run
    std::thread::sleep(Duration::from_secs(2));

    // On Windows ConPTY, dropping master calls ClosePseudoConsole() which blocks
    // while the reader pipe is open, and Child::drop() calls wait() which also
    // blocks. Forget BOTH to avoid all blocking destructors. The child has already
    // run (2s sleep above). The reader thread may leak but process exit cleans up.
    drop(writer);
    std::mem::forget(child);
    std::mem::forget(pair.master);

    // Collect whatever output was produced
    let mut output = Vec::new();
    while let Ok(data) = rx.recv_timeout(Duration::from_millis(500)) {
        output.extend_from_slice(&data);
    }

    output
}

// ─── PTY Tests ───────────────────────────────────────────

/// PTY can spawn a process and capture its stdout.
#[test]
fn pty_spawn_and_read_output() {
    #[cfg(windows)]
    let cmd = {
        let mut c = CommandBuilder::new("cmd");
        c.arg("/C");
        c.arg("echo hello from pty");
        c
    };
    #[cfg(not(windows))]
    let cmd = {
        let mut c = CommandBuilder::new("echo");
        c.arg("hello from pty");
        c
    };

    let output = String::from_utf8_lossy(&spawn_and_collect(cmd)).to_string();
    assert!(
        output.contains("hello from pty"),
        "expected 'hello from pty' in output, got: {:?}",
        output
    );
}

/// PTY output is parseable by vt100 into screen content.
#[test]
fn pty_output_renders_in_vt100() {
    #[cfg(windows)]
    let cmd = {
        let mut c = CommandBuilder::new("cmd");
        c.arg("/C");
        c.arg("echo TESTMARKER-12345");
        c
    };
    #[cfg(not(windows))]
    let cmd = {
        let mut c = CommandBuilder::new("echo");
        c.arg("TESTMARKER-12345");
        c
    };

    let raw = spawn_and_collect(cmd);

    let mut parser = vt100::Parser::new(24, 80, 100);
    parser.process(&raw);

    let screen = parser.screen();
    let mut screen_text = String::new();
    for row in 0..screen.size().0 {
        let row_text = screen.contents_between(row, 0, row, screen.size().1);
        screen_text.push_str(&row_text);
        screen_text.push('\n');
    }

    assert!(
        screen_text.contains("TESTMARKER-12345"),
        "expected TESTMARKER-12345 on vt100 screen, got: {:?}",
        screen_text
    );
}

/// Interactive input via PTY writer reaches the child process.
#[test]
fn pty_write_input_and_read_response() {
    let pty_system = portable_pty::native_pty_system();
    let pair = pty_system.openpty(default_size()).expect("openpty failed");

    #[cfg(windows)]
    let cmd = {
        let mut c = CommandBuilder::new("cmd");
        c.arg("/V:ON");
        c.arg("/C");
        c.arg("set /p X= & echo REPLY:!X!");
        c
    };
    #[cfg(not(windows))]
    let cmd = {
        let mut c = CommandBuilder::new("bash");
        c.arg("-c");
        c.arg("read line && echo REPLY:$line");
        c
    };

    let child = pair.slave.spawn_command(cmd).expect("spawn failed");
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().expect("clone reader failed");
    let mut writer = pair.master.take_writer().expect("take writer failed");

    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // ConPTY sends ESC[6n on startup — respond to unblock output
    std::thread::sleep(Duration::from_millis(200));
    let _ = writer.write_all(b"\x1b[1;1R");
    let _ = writer.flush();

    // Give the process time to start and show its prompt
    std::thread::sleep(Duration::from_millis(500));

    // Write input — this is what interactive mode does
    writer.write_all(b"INPUTTEST\r").expect("write failed");
    writer.flush().expect("flush failed");

    // Let the response come back
    std::thread::sleep(Duration::from_secs(2));

    // Forget both to avoid ConPTY deadlocks (same as spawn_and_collect)
    drop(writer);
    std::mem::forget(child);
    std::mem::forget(pair.master);

    let mut output = Vec::new();
    while let Ok(data) = rx.recv_timeout(Duration::from_millis(500)) {
        output.extend_from_slice(&data);
    }
    let output = String::from_utf8_lossy(&output);

    assert!(
        output.contains("REPLY:INPUTTEST"),
        "expected REPLY:INPUTTEST in output, got: {:?}",
        output
    );
}

/// Multiple PTY processes can run concurrently.
#[test]
fn pty_concurrent_spawn() {
    let handles: Vec<_> = (0..3)
        .map(|i| {
            let marker = format!("CONCURRENT-{}", i);
            std::thread::spawn(move || {
                #[cfg(windows)]
                let cmd = {
                    let mut c = CommandBuilder::new("cmd");
                    c.arg("/C");
                    c.arg(format!("echo {}", marker));
                    c
                };
                #[cfg(not(windows))]
                let cmd = {
                    let mut c = CommandBuilder::new("echo");
                    c.arg(&marker);
                    c
                };

                let raw = spawn_and_collect(cmd);
                let output = String::from_utf8_lossy(&raw).to_string();
                (marker, output)
            })
        })
        .collect();

    for handle in handles {
        let (marker, output) = handle.join().expect("thread panicked");
        assert!(
            output.contains(&marker),
            "expected '{}' in output, got: {:?}",
            marker,
            output
        );
    }
}

// ─── Pure vt100 Tests (no PTY) ──────────────────────────

/// vt100 parser correctly interprets ANSI color escape sequences.
#[test]
fn vt100_parses_ansi_colors() {
    let mut parser = vt100::Parser::new(24, 80, 100);
    parser.process(b"\x1b[31mRED_TEXT\x1b[0m normal_text\r\n");

    let screen = parser.screen();

    let cell_r = screen.cell(0, 0).expect("cell (0,0) should exist");
    assert_eq!(cell_r.contents(), "R");
    assert!(
        matches!(cell_r.fgcolor(), vt100::Color::Idx(1)),
        "expected red (idx 1) foreground, got: {:?}",
        cell_r.fgcolor()
    );

    let cell_n = screen.cell(0, 9).expect("normal_text cell should exist");
    assert_eq!(cell_n.contents(), "n");
    assert!(
        matches!(cell_n.fgcolor(), vt100::Color::Default),
        "expected default foreground after reset, got: {:?}",
        cell_n.fgcolor()
    );
}

/// vt100 parser handles bold, underline, and inverse attributes.
#[test]
fn vt100_parses_text_attributes() {
    let mut parser = vt100::Parser::new(24, 80, 100);
    parser.process(b"\x1b[1mBOLD\x1b[0m \x1b[4mUNDER\x1b[0m \x1b[7mINVERS\x1b[0m\r\n");

    let screen = parser.screen();

    let cell_b = screen.cell(0, 0).unwrap();
    assert!(cell_b.bold(), "expected bold on 'B'");

    let cell_u = screen.cell(0, 5).unwrap();
    assert!(cell_u.underline(), "expected underline on 'U'");

    let cell_i = screen.cell(0, 11).unwrap();
    assert!(cell_i.inverse(), "expected inverse on 'I'");
}

/// vt100 parser resize changes screen dimensions.
#[test]
fn vt100_parser_resize() {
    let mut parser = vt100::Parser::new(24, 80, 100);
    assert_eq!(parser.screen().size(), (24, 80));

    parser.screen_mut().set_size(40, 120);
    assert_eq!(parser.screen().size(), (40, 120));
}

/// Regression: vt100 parser resize preserves visible screen content.
/// Previously, the app replaced the parser with a new instance on resize,
/// which destroyed all buffer content.
#[test]
fn vt100_parser_resize_preserves_scrollback() {
    let mut parser = vt100::Parser::new(10, 80, 100);

    // Write some content to the screen
    for i in 0..5 {
        parser.process(format!("LINE-{}\r\n", i).as_bytes());
    }

    // Verify content is visible before resize
    let contents_before = parser.screen().contents();
    assert!(
        contents_before.contains("LINE-4"),
        "expected LINE-4 on screen before resize, got: {:?}",
        contents_before
    );

    // Resize — this must preserve content (the bug was replacing the parser)
    parser.screen_mut().set_size(12, 100);

    // Verify content survives the resize
    let contents_after = parser.screen().contents();
    assert!(
        contents_after.contains("LINE-4"),
        "content lost after resize: expected LINE-4, got: {:?}",
        contents_after
    );
    assert!(
        contents_after.contains("LINE-0"),
        "content lost after resize: expected LINE-0, got: {:?}",
        contents_after
    );
}
