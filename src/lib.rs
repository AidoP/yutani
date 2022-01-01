#![feature(unix_socket_ancillary_data)]
#![feature(maybe_uninit_uninit_array, maybe_uninit_array_assume_init)]
#![feature(result_option_inspect)]
use std::{
    fmt,
    io,
    os::unix::{net::UnixStream, prelude::RawFd}, collections::VecDeque,
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
        Result,
        RingBuffer
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
    BufferEmpty,
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
            DispatchError::BufferEmpty => write!(f, "Buffer is empty"),
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

/// A ring buffer for use on one thread
/// ```rust
/// use wl::RingBuffer;
/// let data = "Apples, Pears & Oranges".as_bytes();
/// let mut buffer = RingBuffer::new();
/// buffer.write(data).unwrap();
/// assert_eq!(buffer.take(), Some([b'A', b'p', b'p', b'l', b'e', b's']));
/// assert_eq!(buffer.take(), Some([b',', b' ', b'P', b'e', b'a', b'r', b's', b' ', b'&']));
/// buffer.set_writer(RingBuffer::SIZE - 4);
/// buffer.set_reader(RingBuffer::SIZE - 4);
/// buffer.write(data).unwrap();
/// // Will read accross the boundary
/// assert_eq!(buffer.take(), Some([b'A', b'p', b'p', b'l', b'e', b's']));
/// ```
pub struct RingBuffer {
    buffer: [u8; Self::SIZE],
    writer: usize,
    reader: usize
}
impl RingBuffer {
    pub const SIZE: usize = 4096;
    pub const MASK: usize = Self::SIZE - 1;

    pub fn new() -> Self {
        Self {
            buffer: [0; Self::SIZE],
            writer: 0,
            reader: 0
        }
    }
    pub fn is_empty(&self) -> bool {
        self.writer == self.reader
    }
    /// The size of the buffer
    /// ```rust
    /// use wl::RingBuffer;
    /// let mut buffer = RingBuffer::new();
    /// assert_eq!(buffer.len(), 0);
    /// buffer.set_writer(600);
    /// buffer.set_reader(300);
    /// assert_eq!(buffer.len(), 300);
    /// buffer.set_writer(60);
    /// buffer.set_reader(4090);
    /// assert_eq!(buffer.len(), 66);
    /// ```
    pub fn len(&self) -> usize {
        if self.writer >= self.reader {
            self.writer - self.reader
        } else {
            Self::SIZE - self.reader + self.writer
        }
    }
    /// Copy the bytes in data into the buffer, advancing the writer
    /// # Panics
    /// This function will panic if the length of data is more than the remaining available space.
    pub fn write(&mut self, data: &[u8]) -> Option<()> {
        if data.len() > Self::SIZE - self.len() {
            panic!("Cannot write {} bytes into RingBuffer with {} bytes free", data.len(), Self::SIZE - self.len())
        } else {
            let src = data.as_ptr();
            let dst = self.buffer.as_mut_ptr() as *mut u8;
            // Memcpy's are safe assuming reader and writer are less than SIZE
            if self.writer + data.len() <= Self::SIZE {
                unsafe {
                    dst.add(self.writer).copy_from_nonoverlapping(src, data.len());
                }
            } else {
                unsafe {
                    let size = Self::SIZE - self.writer;
                    dst.add(self.writer).copy_from_nonoverlapping(src, size);
                    dst.copy_from_nonoverlapping(src.add(size), data.len() - size);
                }
            }
            self.add_writer(data.len());
            Some(())
        }
    }
    /// Copy `COUNT` bytes out of the array
    /// Returns None if there is not enough data available to fill the array
    pub fn copy<const COUNT: usize>(&self) -> Result<[u8; COUNT]> {
        use std::mem::MaybeUninit;
        let mut buffer: [_; COUNT] = MaybeUninit::uninit_array();
        unsafe {
            self.copy_into_raw(buffer.as_mut_ptr() as *mut u8, COUNT)
            .map(|_| MaybeUninit::array_assume_init(buffer))
        }
    }
    /// Copy `COUNT` bytes out of the array and advance the reader
    #[inline]
    pub fn take<const COUNT: usize>(&mut self) -> Result<[u8; COUNT]> {
        let a = self.copy()?;
        self.add_reader(COUNT);
        Ok(a)
    }
    /// Fills the slice with the next bytes from the buffer
    /// Returns None and leaves the slice as-is if the buffer does not contain enough data to fill the slice
    #[inline]
    pub fn copy_into(&self, slice: &mut [u8]) -> Result<()> {
        unsafe { self.copy_into_raw(slice.as_mut_ptr(), slice.len()) }
    }
    #[inline]
    pub fn take_into(&mut self, slice: &mut [u8]) -> Result<()> {
        self.copy_into(slice)?;
        self.add_reader(slice.len());
        Ok(())
    }
    /// Fills the slice with the next bytes from the buffer
    /// Returns None and leaves the slice as-is if the buffer does not contain enough data to fill the slice
    pub unsafe fn copy_into_raw(&self, dst: *mut u8, count: usize) -> Result<()> {
        if count > self.len() {
            Err(DispatchError::BufferEmpty)
        } else {
            let src = self.buffer.as_ptr();
            if self.reader + count <= Self::SIZE {
                dst.copy_from_nonoverlapping(src.add(self.reader), count);
            } else {
                let size = Self::SIZE - self.reader;
                dst.copy_from_nonoverlapping(src.add(self.reader), size);
                dst.add(size).copy_from_nonoverlapping(src, count - size);
            }
            Ok(())
        }
    }
    #[inline]
    pub unsafe fn take_into_raw(&mut self, dst: *mut u8, count: usize) -> Result<()> {
        self.copy_into_raw(dst, count)?;
        self.add_reader(count);
        Ok(())
    }
    /// Get the reader location
    pub fn reader(&self) -> usize {
        self.reader
    }
    /// Set the reader position, bounded within the buffer size
    pub fn set_reader(&mut self, reader: usize) {
        self.reader = reader.min(Self::SIZE - 1)
    }
    /// Add to the reader, ensuring it does not overtake the writer
    /// ```rust
    /// use wl::RingBuffer;
    /// let mut buffer = RingBuffer::new();
    /// buffer.add_reader(16);
    /// assert_eq!(buffer.reader(), 0);
    /// buffer.set_writer(RingBuffer::SIZE);
    /// buffer.add_reader(16);
    /// assert_eq!(buffer.reader(), 16);
    /// buffer.set_writer(8);
    /// buffer.add_reader(RingBuffer::SIZE);
    /// assert_eq!(buffer.reader(), 8);
    /// ```
    pub fn add_reader(&mut self, count: usize) {
        let count = count.min(self.len());
        self.reader = (self.reader + count) & Self::MASK
    }
    /// Get the writer location 
    pub fn writer(&self) -> usize {
        self.writer
    }
    /// Set the writer position, bounded within the buffer size
    pub fn set_writer(&mut self, writer: usize) {
        self.writer = writer.min(Self::SIZE - 1)
    }
    /// Add to the writer, ensuring it does not overtake the reader
    /// ```rust
    /// use wl::RingBuffer;
    /// let mut buffer = RingBuffer::new();
    /// buffer.add_writer(16);
    /// assert_eq!(buffer.writer(), 16);
    /// buffer.set_reader(32);
    /// buffer.add_writer(RingBuffer::SIZE);
    /// assert_eq!(buffer.writer(), 31);
    /// ```
    pub fn add_writer(&mut self, count: usize) {
        let count = count.min(Self::SIZE - self.len() - 1);
        self.writer = (self.writer + count) & Self::MASK
    }
    /// Fill the buffer with the next message and retrieve ancillary data
    pub fn receive(&mut self, ancillary_data: &mut VecDeque<RawFd>, stream: &UnixStream) -> io::Result<()> {
        use std::io::IoSliceMut;
        use std::os::unix::net::{SocketAncillary, AncillaryData};
        let (a, b) = self.buffer.split_at_mut(self.writer);
        let mut buffer = if self.reader > self.writer {
            [
                IoSliceMut::new(&mut b[..self.reader-self.writer]),
                IoSliceMut::new(&mut [])
            ]
        } else {
            [
                IoSliceMut::new(b),
                IoSliceMut::new(&mut a[..self.reader]),
            ]
        };
        let mut ancillary_buffer = [0; 256];
        let mut ancillary = SocketAncillary::new(&mut ancillary_buffer);
        // What is the state of the buffer on failure?
        let read = stream.recv_vectored_with_ancillary(&mut buffer, &mut ancillary)?;
        self.add_writer(read);
        for message in ancillary.messages() {
            if let Ok(data) = message {
                match data {
                    AncillaryData::ScmRights(fds) => for fd in fds {
                        ancillary_data.push_back(fd)
                    },
                    _ => ()
                }
            }
        }
        Ok(())
    }
}
impl Default for RingBuffer {
    fn default() -> Self {
        Self::new()
    }
}