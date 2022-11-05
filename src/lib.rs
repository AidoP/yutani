use std::path::PathBuf;

use prelude::WlError;

pub mod lease;
pub mod server;
pub mod wire;

pub use prelude::*;
pub mod prelude {
    pub use crate::{Error, lease::Lease, wire::{WlError, EventLoop, Fixed, Id, Message, NewId}};
    pub use syslib::{Fd, File};
}

/// Find a socket that can be opened for listening.
/// 
/// ## Search Order
/// 1. `WAYLAND_DISPLAY` environment variable
/// 2. `$XDG_RUNTIME_DIR/wayland-x` where `x` is a value from `0` to `9`.
/// 3. `wayland.socket`
pub fn find_free_socket() -> PathBuf {
    "wayland.socket".into()
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
    Protocol(WlError<'static>),
    Utf8(std::string::FromUtf8Error),
    Sys(syslib::Error)
}

impl From<syslib::Error> for Error {
    fn from(err: syslib::Error) -> Self {
        Error::Sys(err)
    }
}