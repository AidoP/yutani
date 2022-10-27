use std::{io, fmt, fs::File, collections::VecDeque};

use crate::common::*;

pub struct Client {
    stream: UnixStream,
    messages: RingBuffer,
    fds: VecDeque<File>,
}
impl Client {
    pub fn connect() -> io::Result<Self> {
        let stream = UnixStream::connect(get_socket_path(true)?)?;
        Ok(Self {
            stream,
            messages: RingBuffer::new(),
            fds: Default::default()
        })
    }
    /// Send a message down the wire 
    pub fn send(&mut self, message: Message) -> Result<()> {
        Ok(message.send(&mut self.stream)?)
    }
    /// Get the next available file descriptor from the queue
    pub fn next_file(&mut self) -> std::result::Result<File, DispatchError> {
        self.fds.pop_front().ok_or(DispatchError::ExpectedArgument { data_type: "fd" })
    }
}

pub type Result<T> = std::result::Result<T, Error>;
pub enum Error {
    // /// An error that originates outside of the library, in protocol code
    // Protocol(Box<dyn ErrorHandler>),
    /// An error that occurs during dispatch and can be handled by a user-designated error handler
    Dispatch(DispatchError),
    /// An error indicating that the connection to the client must be severed
    System(SystemError)
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Self::Protocol(error) => write!(f, "Protocol error could not be handled, {}", error),
            Self::Dispatch(error) => write!(f, "Error during internal message handling, {}", error),
            Self::System(error) => write!(f, "Unrecoverable error, {}", error)
        }
    }
}
impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl From<SystemError> for Error {
    fn from(error: SystemError) -> Self {
        Error::System(error)
    }
}
impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::System(error.into())
    }
}
impl From<DispatchError> for Error {
    fn from(error: DispatchError) -> Self {
        Error::Dispatch(error)
    }
}