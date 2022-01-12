#![feature(maybe_uninit_uninit_array, maybe_uninit_array_assume_init)]
#![feature(io_error_more)]
#![feature(box_syntax)]
#![feature(result_flattening)]
#![feature(unsize)]
#![feature(coerce_unsized)]
#![feature(dispatch_from_dyn)]
use std::{fmt, io};

pub mod server;
pub use server::Server;

mod types;
pub use types::{Fixed, NewId, Fd, Array};

mod message;
pub use message::Message;

pub mod socket;

mod common {
    use std::env;
    pub use crate::{
        types::*,
        socket::*,
        DispatchError,
        message::Message,
        Result,
        RingBuffer,
        Object
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
lazy_static::lazy_static! {
    /// Indicates that messages should debug-print
    pub static ref DEBUG: bool = cfg!(debug_assertions) || std::env::var("WAYLAND_DEBUG").is_ok();
}

/// An item that represents an object
pub trait Object: fmt::Display {
    fn object(&self) -> u32;
}
impl Object for u32 {
    fn object(&self) -> u32 {
        *self
    }
}

pub type Result<T> = std::result::Result<T, DispatchError>;
#[derive(Debug)]
pub enum DispatchError {
    IOError(io::Error),
    BufferEmpty,
    ObjectNull,
    ObjectLeased(u32),
    ObjectExists(u32),
    ObjectNotFound(u32),
    EnumVariantInvalid(&'static str, u32),
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
            DispatchError::ObjectNull => write!(f, "Null object accessed"),
            DispatchError::ObjectLeased(object_id) => write!(f, "Object {} already in use", object_id),
            DispatchError::ObjectExists(object_id) => write!(f, "Cannot create object {} as it already exists", object_id),
            DispatchError::ObjectNotFound(object_id) => write!(f, "Object {} does not exist", object_id),
            DispatchError::EnumVariantInvalid(enum_name, variant) => write!(f, "Enum {:?} has no variant \"{}\"", enum_name, variant),
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
    // Must be a power of 2 for the mask to work
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
    pub(crate) fn iov_mut(&mut self) -> [socket::IoVec; 2] {
        use socket::IoVec;
        let (a, b) = self.buffer.split_at_mut(self.writer);
        if self.reader > self.writer {
            [
                IoVec::from(&mut b[..self.reader-self.writer]),
                IoVec::empty()
            ]
        } else {
            [
                IoVec::from(b),
                IoVec::from(&mut a[..self.reader]),
            ]
        }
    }
}
impl Default for RingBuffer {
    fn default() -> Self {
        Self::new()
    }
}