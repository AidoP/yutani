pub mod lease;
pub mod server;
pub mod wire;

pub mod prelude {
    pub use crate::{Error, lease::Lease, wire::{WlError, EventLoop, Id, Message, NewId}};
    pub(crate) use syslib::Fd;
}

pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug)]
pub enum Error {
    InvalidSocketPath,
    DoubleLease,
    BufferEmpty,
    NoGlobal,
    UnsupportedVersion(&'static str, u32),
    NoObject(u32),
    DuplicateObject(u32),
    Utf8(std::string::FromUtf8Error),
    Sys(syslib::Error)
}

impl From<syslib::Error> for Error {
    fn from(err: syslib::Error) -> Self {
        Error::Sys(err)
    }
}