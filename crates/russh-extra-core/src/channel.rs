//! Channel-related domain types.

use std::collections::BTreeMap;

/// Default captured stdout or stderr limit for buffered commands.
pub const DEFAULT_COMMAND_OUTPUT_LIMIT: usize = 8 * 1024 * 1024;

/// High-level SSH channel kind.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ChannelKind {
    /// Session channel.
    Session,
    /// Direct TCP/IP channel.
    DirectTcpIp,
    /// Forwarded TCP/IP channel.
    ForwardedTcpIp,
    /// Direct streamlocal channel.
    DirectStreamLocal,
    /// Forwarded streamlocal channel.
    ForwardedStreamLocal,
    /// Named subsystem channel.
    Subsystem(String),
}

/// Remote command exit information.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandExit {
    /// The remote process reported an exit status.
    Status(u32),
    /// The remote process was terminated by a signal.
    Signal(String),
    /// The channel closed without an exit status or signal.
    Missing,
}

/// Captured output limits for buffered command execution.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandLimits {
    stdout: usize,
    stderr: usize,
}

impl CommandLimits {
    /// Creates command output limits.
    pub fn new(stdout: usize, stderr: usize) -> Self {
        Self { stdout, stderr }
    }

    /// Returns the stdout byte limit.
    pub fn stdout(&self) -> usize {
        self.stdout
    }

    /// Returns the stderr byte limit.
    pub fn stderr(&self) -> usize {
        self.stderr
    }

    /// Sets the stdout byte limit.
    pub fn with_stdout(mut self, stdout: usize) -> Self {
        self.stdout = stdout;
        self
    }

    /// Sets the stderr byte limit.
    pub fn with_stderr(mut self, stderr: usize) -> Self {
        self.stderr = stderr;
        self
    }
}

impl Default for CommandLimits {
    fn default() -> Self {
        Self {
            stdout: DEFAULT_COMMAND_OUTPUT_LIMIT,
            stderr: DEFAULT_COMMAND_OUTPUT_LIMIT,
        }
    }
}

impl CommandExit {
    /// Creates a command exit from an exit status.
    pub fn status(status: u32) -> Self {
        Self::Status(status)
    }

    /// Creates a command exit from a signal name.
    pub fn signal(signal: impl Into<String>) -> Self {
        Self::Signal(signal.into())
    }

    /// Creates a command exit for a channel that closed without status.
    pub fn missing() -> Self {
        Self::Missing
    }

    /// Returns the optional exit status.
    pub fn code(&self) -> Option<u32> {
        match self {
            Self::Status(status) => Some(*status),
            Self::Signal(_) | Self::Missing => None,
        }
    }

    /// Returns the optional signal name.
    pub fn signal_name(&self) -> Option<&str> {
        match self {
            Self::Signal(signal) => Some(signal),
            Self::Status(_) | Self::Missing => None,
        }
    }

    /// Returns whether the command exited successfully.
    pub fn success(&self) -> bool {
        matches!(self, Self::Status(0))
    }
}

/// Pseudo-terminal request.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pty {
    term: String,
    width_columns: u32,
    height_rows: u32,
    width_pixels: u32,
    height_pixels: u32,
    modes: BTreeMap<TerminalMode, u32>,
}

impl Pty {
    /// Creates a PTY request.
    pub fn new(term: impl Into<String>, width_columns: u32, height_rows: u32) -> Self {
        Self {
            term: term.into(),
            width_columns,
            height_rows,
            width_pixels: 0,
            height_pixels: 0,
            modes: BTreeMap::new(),
        }
    }

    /// Returns the terminal type.
    pub fn term(&self) -> &str {
        &self.term
    }

    /// Returns terminal width in columns.
    pub fn width_columns(&self) -> u32 {
        self.width_columns
    }

    /// Returns terminal height in rows.
    pub fn height_rows(&self) -> u32 {
        self.height_rows
    }

    /// Returns terminal width in pixels.
    pub fn width_pixels(&self) -> u32 {
        self.width_pixels
    }

    /// Returns terminal height in pixels.
    pub fn height_pixels(&self) -> u32 {
        self.height_pixels
    }

    /// Sets pixel dimensions.
    pub fn with_pixels(mut self, width_pixels: u32, height_pixels: u32) -> Self {
        self.width_pixels = width_pixels;
        self.height_pixels = height_pixels;
        self
    }

    /// Sets a terminal mode.
    pub fn with_mode(mut self, mode: TerminalMode, value: u32) -> Self {
        self.modes.insert(mode, value);
        self
    }

    /// Returns terminal modes.
    pub fn modes(&self) -> &BTreeMap<TerminalMode, u32> {
        &self.modes
    }
}

impl Default for Pty {
    fn default() -> Self {
        Self::new("xterm-256color", 80, 24)
    }
}

/// Terminal mode opcode.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum TerminalMode {
    /// Interrupt character.
    Interrupt,
    /// Quit character.
    Quit,
    /// Erase character.
    Erase,
    /// Kill character.
    Kill,
    /// End-of-file character.
    EndOfFile,
    /// Terminal input speed.
    InputSpeed,
    /// Terminal output speed.
    OutputSpeed,
    /// Custom opcode.
    Custom(u8),
}

#[cfg(test)]
mod tests {
    use crate::{CommandLimits, DEFAULT_COMMAND_OUTPUT_LIMIT};

    #[test]
    fn command_limits_default_to_eight_mib_per_stream() {
        let limits = CommandLimits::default();

        assert_eq!(limits.stdout(), DEFAULT_COMMAND_OUTPUT_LIMIT);
        assert_eq!(limits.stderr(), DEFAULT_COMMAND_OUTPUT_LIMIT);
    }

    #[test]
    fn command_limits_are_configurable() {
        let limits = CommandLimits::default().with_stdout(1024).with_stderr(2048);

        assert_eq!(limits.stdout(), 1024);
        assert_eq!(limits.stderr(), 2048);
    }
}
