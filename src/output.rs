use std::io::{self, Write};

use anyhow::Result;
use serde_json::Value;

/// Line-delimited, flushed events for desktop clients. Human output stays the default.
pub struct Output {
    pub json: bool,
}

impl Output {
    pub fn event(&self, value: Value) -> Result<()> {
        if self.json {
            let mut stdout = io::stdout().lock();
            serde_json::to_writer(&mut stdout, &value)?;
            writeln!(stdout)?;
            stdout.flush()?;
        }
        Ok(())
    }

    pub fn message(&self, args: std::fmt::Arguments<'_>) -> Result<()> {
        if !self.json {
            let mut stdout = io::stdout().lock();
            writeln!(stdout, "{args}")?;
            stdout.flush()?;
        }
        Ok(())
    }
}
