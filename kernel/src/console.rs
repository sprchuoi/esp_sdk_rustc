//! Line-oriented console framework.
//!
//! The transport (USB-Serial-JTAG or UART) is the SoC's job; this module owns the
//! command table, the parser and the help output. Commands are plain function
//! pointers so registration is static and allocation-free, and the output goes
//! through [`core::fmt::Write`], which means the same commands work over a UART,
//! a defmt sink or an in-memory buffer in tests.

use core::fmt;

/// Maximum arguments a command can receive.
pub const MAX_ARGS: usize = 8;

/// A sink for console output.
pub trait ConsoleIo: fmt::Write {}

impl<T: fmt::Write + ?Sized> ConsoleIo for T {}

/// One console command.
#[derive(Clone, Copy)]
pub struct Command {
    /// Command name, e.g. `sched`.
    pub name: &'static str,
    /// One-line help text.
    pub help: &'static str,
    /// Handler: receives arguments (without the name) and the output sink.
    pub run: fn(&[&str], &mut dyn ConsoleIo),
}

impl Command {
    /// Define a command.
    #[must_use]
    pub const fn new(
        name: &'static str,
        help: &'static str,
        run: fn(&[&str], &mut dyn ConsoleIo),
    ) -> Self {
        Self { name, help, run }
    }
}

/// Why a console operation failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsoleError {
    /// The command table is full.
    Capacity,
    /// A command with this name is already registered.
    Duplicate,
    /// The name is empty or contains whitespace.
    InvalidName,
    /// The input line was empty.
    Empty,
    /// The command is not registered.
    Unknown,
    /// More than [`MAX_ARGS`] arguments were supplied.
    TooManyArgs,
}

/// Static command table.
#[derive(Clone)]
pub struct Console<const N: usize> {
    commands: [Option<Command>; N],
    count: usize,
}

impl<const N: usize> Default for Console<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Console<N> {
    /// Create an empty console.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            commands: [None; N],
            count: 0,
        }
    }

    /// Number of registered commands.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }

    /// Whether no command is registered.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Register a command.
    ///
    /// # Errors
    /// [`ConsoleError::InvalidName`] for an empty/whitespace name,
    /// [`ConsoleError::Duplicate`] for a repeated name, [`ConsoleError::Capacity`]
    /// when the table is full.
    pub fn register(&mut self, command: Command) -> Result<(), ConsoleError> {
        if command.name.is_empty() || command.name.chars().any(char::is_whitespace) {
            return Err(ConsoleError::InvalidName);
        }
        if self.find(command.name).is_some() {
            return Err(ConsoleError::Duplicate);
        }
        if self.count >= N {
            return Err(ConsoleError::Capacity);
        }
        self.commands[self.count] = Some(command);
        self.count += 1;
        Ok(())
    }

    /// Find a command by name.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<&Command> {
        self.commands[..self.count]
            .iter()
            .flatten()
            .find(|command| command.name == name)
    }

    /// Execute one input line.
    ///
    /// `help` is built in and lists every registered command.
    ///
    /// # Errors
    /// See [`ConsoleError`]; parse errors are also written to `out`.
    pub fn execute(&mut self, line: &str, out: &mut dyn ConsoleIo) -> Result<(), ConsoleError> {
        let line = line.trim();
        if line.is_empty() {
            return Err(ConsoleError::Empty);
        }

        let mut words = line.split_whitespace();
        let name = words.next().ok_or(ConsoleError::Empty)?;
        let mut args = [""; MAX_ARGS];
        let mut argc = 0;
        for word in words {
            if argc == MAX_ARGS {
                let _ = writeln!(out, "too many arguments (max {MAX_ARGS})");
                return Err(ConsoleError::TooManyArgs);
            }
            args[argc] = word;
            argc += 1;
        }

        if name == "help" {
            self.help(out);
            return Ok(());
        }

        match self.find(name) {
            Some(command) => {
                (command.run)(&args[..argc], out);
                Ok(())
            }
            None => {
                let _ = writeln!(out, "unknown command: {name} (try 'help')");
                Err(ConsoleError::Unknown)
            }
        }
    }

    /// Print the command list.
    pub fn help(&self, out: &mut dyn ConsoleIo) {
        let _ = writeln!(out, "commands:");
        for command in self.commands[..self.count].iter().flatten() {
            let _ = writeln!(out, "  {:<10} {}", command.name, command.help);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::string::String;

    fn echo(args: &[&str], out: &mut dyn ConsoleIo) {
        for (index, arg) in args.iter().enumerate() {
            if index > 0 {
                let _ = write!(out, " ");
            }
            let _ = write!(out, "{arg}");
        }
        let _ = writeln!(out);
    }

    fn noop(_args: &[&str], _out: &mut dyn ConsoleIo) {}

    fn console() -> Console<4> {
        let mut console = Console::new();
        console
            .register(Command::new("echo", "print arguments", echo))
            .unwrap();
        console
            .register(Command::new("sched", "show tasks", noop))
            .unwrap();
        console
    }

    #[test]
    fn registers_and_finds_commands() {
        let console = console();
        assert_eq!(console.len(), 2);
        assert!(console.find("echo").is_some());
        assert!(console.find("missing").is_none());
    }

    #[test]
    fn rejects_invalid_and_duplicate_names() {
        let mut console = Console::<2>::new();
        assert_eq!(
            console.register(Command::new("", "x", noop)),
            Err(ConsoleError::InvalidName)
        );
        assert_eq!(
            console.register(Command::new("two words", "x", noop)),
            Err(ConsoleError::InvalidName)
        );
        console.register(Command::new("a", "x", noop)).unwrap();
        assert_eq!(
            console.register(Command::new("a", "x", noop)),
            Err(ConsoleError::Duplicate)
        );
        console.register(Command::new("b", "x", noop)).unwrap();
        assert_eq!(
            console.register(Command::new("c", "x", noop)),
            Err(ConsoleError::Capacity)
        );
    }

    #[test]
    fn executes_with_arguments() {
        let mut console = console();
        let mut out = String::new();
        console.execute("echo hello world", &mut out).unwrap();
        assert_eq!(out, "hello world\n");
    }

    #[test]
    fn rejects_empty_and_unknown() {
        let mut console = console();
        let mut out = String::new();
        assert_eq!(console.execute("   ", &mut out), Err(ConsoleError::Empty));
        assert_eq!(
            console.execute("nope", &mut out),
            Err(ConsoleError::Unknown)
        );
        assert!(out.contains("unknown command: nope"));
    }

    #[test]
    fn help_lists_commands() {
        let mut console = console();
        let mut out = String::new();
        console.execute("help", &mut out).unwrap();
        assert!(out.contains("echo"));
        assert!(out.contains("print arguments"));
        assert!(out.contains("sched"));
    }

    #[test]
    fn refuses_too_many_arguments() {
        let mut console = console();
        let mut out = String::new();
        let line = "echo 1 2 3 4 5 6 7 8 9";
        assert_eq!(
            console.execute(line, &mut out),
            Err(ConsoleError::TooManyArgs)
        );
        assert!(out.contains("too many arguments"));
    }
}
