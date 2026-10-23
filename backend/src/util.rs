use std::io::{self, Write};

pub fn log_stderr(message: impl AsRef<str>) {
    let _ = writeln!(io::stderr(), "[io.dbx.process] {}", message.as_ref());
}
