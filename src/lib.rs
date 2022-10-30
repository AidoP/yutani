pub mod lease;
pub mod server;
pub mod wire;

pub mod prelude {
    pub use crate::{Error, lease::*, Result, wire::{EventLoop, Id, NewId}};
    pub(crate) use syslib::Fd;
}

pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug)]
pub enum Error {
    InvalidSocketPath,
    DoubleLease,
    NoObject(u32),
    DuplicateObject(u32),
    Sys(syslib::Error)
}

impl From<syslib::Error> for Error {
    fn from(err: syslib::Error) -> Self {
        Error::Sys(err)
    }
}