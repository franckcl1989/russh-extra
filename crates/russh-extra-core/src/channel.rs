//! Channel-related domain types.

use std::collections::BTreeMap;

/// Default captured stdout or stderr limit for buffered commands.
pub const DEFAULT_COMMAND_OUTPUT_LIMIT: usize = 8 * 1024 * 1024;

/// High-level SSH channel kind.
#[non_exhaustive]
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
    /// X11 forwarding channel.
    X11,
    /// SSH agent forwarding channel.
    AuthAgent,
}

impl ChannelKind {
    /// Creates a session channel kind.
    pub fn session() -> Self {
        Self::Session
    }

    /// Creates a direct TCP/IP channel kind.
    pub fn direct_tcp_ip() -> Self {
        Self::DirectTcpIp
    }

    /// Creates a forwarded TCP/IP channel kind.
    pub fn forwarded_tcp_ip() -> Self {
        Self::ForwardedTcpIp
    }

    /// Creates a direct streamlocal channel kind.
    pub fn direct_stream_local() -> Self {
        Self::DirectStreamLocal
    }

    /// Creates a forwarded streamlocal channel kind.
    pub fn forwarded_stream_local() -> Self {
        Self::ForwardedStreamLocal
    }

    /// Creates a subsystem channel kind.
    pub fn subsystem(name: impl Into<String>) -> Self {
        Self::Subsystem(name.into())
    }

    /// Creates an X11 forwarding channel kind.
    pub fn x11() -> Self {
        Self::X11
    }

    /// Creates an SSH agent forwarding channel kind.
    pub fn auth_agent() -> Self {
        Self::AuthAgent
    }
}

/// Remote command exit information.
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandExit {
    /// The remote process reported an exit status.
    Status(u32),
    /// The remote process was terminated by a signal.
    Signal(String, bool),
    /// The channel closed without an exit status or signal.
    Missing,
}

/// Captured output limits for buffered command execution.
#[non_exhaustive]
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
    pub fn signal(signal: impl Into<String>, core_dumped: bool) -> Self {
        Self::Signal(signal.into(), core_dumped)
    }

    /// Creates a command exit for a channel that closed without status.
    pub fn missing() -> Self {
        Self::Missing
    }

    /// Returns the optional exit status.
    pub fn code(&self) -> Option<u32> {
        match self {
            Self::Status(status) => Some(*status),
            Self::Signal(..) | Self::Missing => None,
        }
    }

    /// Returns the optional signal name.
    pub fn signal_name(&self) -> Option<&str> {
        match self {
            Self::Signal(signal, _) => Some(signal),
            Self::Status(_) | Self::Missing => None,
        }
    }

    /// Returns whether the command exited successfully.
    pub fn success(&self) -> bool {
        matches!(self, Self::Status(0))
    }
}

/// Pseudo-terminal request.
#[non_exhaustive]
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

    /// Sets the terminal type.
    pub fn with_term(mut self, term: impl Into<String>) -> Self {
        self.term = term.into();
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
#[non_exhaustive]
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
    /// Echo characters.
    Echo,
    /// Erase previous character on display.
    EchoErase,
    /// Erase line on display.
    EchoKill,
    /// Echo newlines on display.
    EchoNl,
    /// Canonical input mode.
    CanonicalInput,
    /// Signal checking.
    SigCheck,
    /// Map CR to NL on input.
    CrToNlInput,
    /// Map NL to CR on input.
    NlToCrInput,
    /// Ignore CR on input.
    IgnoreCrInput,
    /// Post-process output.
    PostProcessOutput,
    /// Map NL to CR-NL on output.
    NlToCrNlOutput,
    /// Map CR to NL on output.
    CrToNlOutput,
    /// Do not output CR on NL.
    NoCrOnNl,
    /// Custom opcode.
    Custom(u8),
}

#[cfg(test)]
mod tests {
    use crate::{ChannelKind, CommandExit, CommandLimits, DEFAULT_COMMAND_OUTPUT_LIMIT, Pty};

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

    #[test]
    fn pty_with_term_sets_terminal_type() {
        let pty = Pty::default().with_term("vt100");
        assert_eq!(pty.term(), "vt100");
    }

    #[test]
    fn pty_with_term_overrides_default() {
        let pty = Pty::new("screen", 120, 40).with_term("xterm");
        assert_eq!(pty.term(), "xterm");
    }

    #[test]
    fn channel_kind_has_x11_and_auth_agent_variants() {
        assert_eq!(ChannelKind::X11, ChannelKind::X11);
        assert_eq!(ChannelKind::AuthAgent, ChannelKind::AuthAgent);
        assert_ne!(ChannelKind::X11, ChannelKind::AuthAgent);
        assert_ne!(ChannelKind::Session, ChannelKind::X11);
    }

    #[test]
    fn channel_kind_constructors() {
        assert_eq!(ChannelKind::session(), ChannelKind::Session);
        assert_eq!(ChannelKind::direct_tcp_ip(), ChannelKind::DirectTcpIp);
        assert_eq!(ChannelKind::forwarded_tcp_ip(), ChannelKind::ForwardedTcpIp);
        assert_eq!(
            ChannelKind::direct_stream_local(),
            ChannelKind::DirectStreamLocal
        );
        assert_eq!(
            ChannelKind::forwarded_stream_local(),
            ChannelKind::ForwardedStreamLocal
        );
        assert_eq!(
            ChannelKind::subsystem("sftp"),
            ChannelKind::Subsystem("sftp".to_owned())
        );
        assert_eq!(ChannelKind::x11(), ChannelKind::X11);
        assert_eq!(ChannelKind::auth_agent(), ChannelKind::AuthAgent);
    }

    #[test]
    fn command_exit_signal_stores_core_dumped_true() {
        let exit = CommandExit::signal("ABRT", true);
        assert_eq!(exit.signal_name(), Some("ABRT"));
        assert!(matches!(exit, CommandExit::Signal(_, true)));
    }

    #[test]
    fn command_exit_signal_stores_core_dumped_false() {
        let exit = CommandExit::signal("TERM", false);
        assert_eq!(exit.signal_name(), Some("TERM"));
        assert!(matches!(exit, CommandExit::Signal(_, false)));
    }

    #[test]
    fn command_exit_code_ignores_signal_variants() {
        assert_eq!(CommandExit::signal("KILL", false).code(), None);
        assert_eq!(CommandExit::signal("KILL", true).code(), None);
    }
}
