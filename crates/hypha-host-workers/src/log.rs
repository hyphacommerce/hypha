//! `tracing` to the Workers console (`console.log`), which Workers Logs and `wrangler
//! tail` collect. No timestamps: the platform stamps each line.

use std::io::Write;

use tracing_subscriber::fmt::MakeWriter;

/// One event's line, written to the console when it's done.
pub struct Line(Vec<u8>);

impl Write for Line {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for Line {
    fn drop(&mut self) {
        let line = String::from_utf8_lossy(&self.0);
        worker::console_log!("{}", line.trim_end());
    }
}

struct Console;

impl<'a> MakeWriter<'a> for Console {
    type Writer = Line;

    fn make_writer(&'a self) -> Line {
        Line(Vec::new())
    }
}

/// Sends events at `level` and above to the console; once per isolate (later calls do
/// nothing).
pub fn init(level: tracing::Level) {
    let _ = tracing_subscriber::fmt()
        .with_writer(Console)
        .with_max_level(level)
        .without_time()
        .with_ansi(false)
        .try_init();
}
