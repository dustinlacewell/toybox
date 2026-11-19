use std::fmt;

/// A JavaScript failure that reached Rust: a thrown value, a rejected promise,
/// a module that could not be loaded, an interrupt, or a deadlock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// The error's `name` (`TypeError`, `Error`, ...). Empty when not an Error object.
    pub name: String,
    /// The error's `message`.
    pub message: String,
    /// The JavaScript stack (`at fn (file:line:col)` lines), when there is one.
    pub stack: Option<String>,
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Error { name: String::new(), message: message.into(), stack: None }
    }

    pub(crate) fn interrupted(previous: Option<Error>) -> Self {
        Error {
            name: "InterruptError".into(),
            message: "Script execution was interrupted (the host raised the interrupt flag)".into(),
            stack: previous.and_then(|e| e.stack),
        }
    }

    /// True for the error produced when the interrupt flag stopped the script.
    pub fn is_interrupt(&self) -> bool {
        self.name == "InterruptError"
    }
}

impl fmt::Display for Error {
    /// `Name: message`, followed by the stack lines when present.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.name.is_empty() && self.name != "Error" {
            write!(f, "{}: ", self.name)?;
        }
        write!(f, "{}", self.message)?;
        if let Some(stack) = &self.stack {
            let stack = stack.trim_end();
            if !stack.is_empty() {
                write!(f, "\n{stack}")?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

impl From<String> for Error {
    fn from(message: String) -> Self {
        Error::new(message)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
