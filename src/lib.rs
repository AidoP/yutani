use std::{
    fmt,
    io,
    os::unix::net::UnixStream,
};

pub mod server;
pub use server::Server;

mod types;
pub use types::{Fixed, NewId};

mod message;
pub use message::Message;

mod common {
    use std::env;
    pub use crate::{
        types::*,
        DispatchError,
        message::Message,
        Result
    };

    pub fn get_socket_path() -> String {
        if let Ok(path ) = env::var("WAYLAND_DISPLAY") {
            path
        } else {
            if let Ok(path) = env::var("XDG_RUNTIME_DIR") {
                path + "/wayland-0"
            } else {
                "wayland-0".into()
            }
        }
    }
}

pub struct Client(UnixStream);
impl Client {
    pub fn connect() -> io::Result<Self> {
        UnixStream::connect(common::get_socket_path()).map(|socket| Self(socket))
    }
    pub fn send(&mut self, bytes: &[u8]) {
        use io::Write;
        self.0.write_all(bytes).unwrap();
        self.0.flush().unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
        self.0.shutdown(std::net::Shutdown::Both).unwrap();
    }
}


pub type Result<T> = std::result::Result<T, DispatchError>;
#[derive(Debug)]
pub enum DispatchError {
    IOError(io::Error),
    ObjectTaken(u32),
    ObjectExists(u32),
    ObjectNotFound(u32),
    InvalidOpcode(u32, u16, &'static str),
    InvalidObject(&'static str, &'static str),
    ExpectedArgument(&'static str),
    Utf8Error(std::str::Utf8Error, &'static str)
}
impl fmt::Display for DispatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DispatchError::IOError(e) => write!(f, "{}", e),
            DispatchError::ObjectTaken(object_id) => write!(f, "Object {} already in use", object_id),
            DispatchError::ObjectExists(object_id) => write!(f, "Cannot create object {} as it already exists", object_id),
            DispatchError::ObjectNotFound(object_id) => write!(f, "Object {} does not exist", object_id),
            DispatchError::InvalidOpcode(object_id, opcode, interface) => write!(f, "Opcode {} is invalid for object {} implementing interface `{}`", opcode, object_id, interface),
            DispatchError::InvalidObject(expected, got) => write!(f, "Expected object of interface `{}` but instead got `{}`", expected, got),
            DispatchError::ExpectedArgument(argument) => write!(f, "Expected argument of type {}", argument),
            DispatchError::Utf8Error(error, reason) => write!(f, "{} is not a UTF-8 string: {:?}", reason, error)
        }
    }
}
impl From<io::Error> for DispatchError {
    fn from(error: io::Error) -> Self {
        DispatchError::IOError(error)
    }
}