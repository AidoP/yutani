use std::{fmt::Debug, path::Path, ops::{Deref, DerefMut}, borrow::Cow};

use crate::{prelude::*};
use ahash::{HashMap, HashMapExt};
use syslib::{Socket, File, FileDescriptor};

#[derive(Debug)]
pub struct WlError<'a> {
    pub object: Id,
    pub error: u32,
    pub description: Cow<'a, str>
}
impl<'a> WlError<'a> {
    pub const CORRUPT: Self = Self {
        object: Id(1),
        error: 1,
        description: Cow::Borrowed("Protocol violation or malformed request.")
    };
    pub const NO_OBJECT: Self = Self {
        object: Id(1),
        error: 0,
        description: Cow::Borrowed("No object with that ID.")
    };
    pub const NO_GLOBAL: Self = Self {
        object: Id(1),
        error: 1,
        description: Cow::Borrowed("Invalid request for a global.")
    };
    pub const UTF_8: Self = Self {
        object: Id(1),
        error: 1,
        description: Cow::Borrowed("Strings must be valid UTF-8.")
    };
    pub const NO_FD: Self = Self {
        object: Id(1),
        error: 1,
        description: Cow::Borrowed("Expected a file descriptor but none were received.")
    };
    pub const OOM: Self = Self {
        object: Id(1),
        error: 2,
        description: Cow::Borrowed("The compositor is out of memory.")
    };
    pub const INTERNAL: Self = Self {
        object: Id(1),
        error: 3,
        description: Cow::Borrowed("Compositor state is corrupted.")
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Id(u32);
impl Id {
    /// The display object that must always exist for Wayland to operate.
    pub const DISPLAY: Self = Self(1);
}
impl From<u32> for Id {
    fn from(id: u32) -> Self {
        Self(id)
    }
}
impl Into<u32> for Id {
    fn into(self) -> u32 {
        self.0
    }
}
pub struct NewId {
    id: Id,
    version: u32,
    interface: String
}
impl NewId {
    #[inline]
    pub fn id(&self) -> Id {
        self.id
    }
    #[inline]
    pub fn version(&self) -> u32 {
        self.version
    }
    #[inline]
    pub fn interface(&self) -> &str {
        &self.interface
    }
}
/// Fixed decimal number as specified by the Wayland wire format
#[repr(transparent)]
pub struct Fixed(u32);
impl Fixed {
    #[inline]
    fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
}

#[derive(Debug)]
pub struct Message {
    pub object: Id,
    pub opcode: u16,
    pub size: u16
}
/// Used to complete a message, preventing new arguments from being pushed.
#[must_use]
#[derive(Debug)]
pub struct CommitKey(usize);

pub trait EventSource<T> {
    fn fd(&self) -> Fd<'static>;
    fn destroy(&mut self, _event_loop: &mut EventLoop<T>) {}
    fn input(&mut self, event_loop: &mut EventLoop<T>) -> crate::Result<()>;
}
pub struct EventLoop<T> {
    epoll: File,
    sources: HashMap<u32, Option<Box<dyn EventSource<T>>>>,
    pub state: T
}
impl<T> EventLoop<T> {
    pub fn new(state: T) -> crate::Result<Self> {
        Ok(Self {
            epoll: syslib::epoll_create(syslib::epoll::Flags::CLOSE_ON_EXEC)?,
            sources: HashMap::new(),
            state
        })
    }
    pub fn add(&mut self, event_source: Box<dyn EventSource<T>>) -> crate::Result<()> {
        use syslib::epoll;
        let fd = event_source.fd();
        let event = epoll::Event {
            events: epoll::Events::INPUT | epoll::Events::ERROR | epoll::Events::HANG_UP,
            data: epoll::Data { fd }
        };
        syslib::epoll_ctl(&self.epoll, &fd, epoll::Cntl::Add(event))?;
        self.sources.insert(fd.raw(), Some(event_source));
        Ok(())
    }
    pub fn wait(&mut self, timeout: u32) -> crate::Result<()> {
        use syslib::epoll;
        let mut events: [MaybeUninit<epoll::Event>; 32] = std::array::from_fn(|_| std::mem::MaybeUninit::uninit());
        let events = syslib::epoll_wait(&self.epoll, &mut events, timeout)?;
        for event in events {
            let fd = unsafe { event.data.fd };
            let mut had_error = false;
            if event.events.any(epoll::Events::INPUT) {
                // Lease the event source so that it can modify its owning data structure
                let mut source = self.sources.get_mut(&fd.raw()).unwrap().take();
                if let Err(err) = source.as_mut().unwrap().input(self) {
                    #[cfg(debug_assertions)]
                    eprintln!("Dropping event {:?}: {:?}", fd, err);
                    had_error = true;
                }
                let leased_source = self.sources.get_mut(&fd.raw())
                    .expect("An event source erroneously removed it's own entry.");
                // Return the lease of the event source
                std::mem::swap(&mut source, leased_source)
            }
            if event.events.any(epoll::Events::ERROR | epoll::Events::HANG_UP) || had_error {
                syslib::epoll_ctl(&self.epoll, &fd, epoll::Cntl::Delete)?;
                let source = self.sources.remove(&fd.raw());
                source.unwrap().unwrap().destroy(self);
            }
        }
        Ok(())
    }
}
impl<T> Deref for EventLoop<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.state
    }
}
impl<T> DerefMut for EventLoop<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.state
    }
}

pub struct Server {
    pub(crate) socket: Socket
}
impl Server {
    pub fn listen<P: AsRef<Path>>(path: P) -> crate::Result<Self> {
        use std::os::unix::prelude::OsStrExt;
        use syslib::sock::*;
        let socket = syslib::socket(Domain::UNIX, Type::STREAM | TypeFlags::CLOSE_ON_EXEC, Protocol::UNSPECIFIED)?;
        let address = UnixAddress::new(path.as_ref().as_os_str().as_bytes()).map_err(|_| Error::InvalidSocketPath)?;
        syslib::bind(&socket, address.address())?;
        syslib::listen(&socket, syslib::sock::MAX_CONNECTIONS)?;

        Ok(Self {
            socket
        })
    }
}

pub struct Stream {
    pub(crate) socket: Socket,
    rx_msg: RingBuffer<u32>,
    tx_msg: RingBuffer<u32>,
    rx_fd: RingBuffer<File>,
    tx_fd: RingBuffer<Fd<'static>>,
}
impl Stream {
    /// Open a new stream connected to a Unix domain socket.
    /// 
    /// `path` Must be less than 108 bytes long.
    pub fn connect<P: AsRef<Path>>(path: P) -> crate::Result<Self> {
        use std::os::unix::prelude::OsStrExt;
        use syslib::sock::*;
        let socket = syslib::socket(Domain::UNIX, Type::STREAM | TypeFlags::CLOSE_ON_EXEC, Protocol::UNSPECIFIED)?;
        let address = UnixAddress::new(path.as_ref().as_os_str().as_bytes()).map_err(|_| Error::InvalidSocketPath)?;
        syslib::connect(&socket, address.address())?;

        Self::new(socket)
    }
    pub(crate) fn new(socket: Socket) -> crate::Result<Self> {
        let flags: syslib::open::Flags = syslib::fcntl(&socket, syslib::Fcntl::GetFd)?.try_into()?;
        syslib::fcntl(&socket, syslib::Fcntl::SetFd(flags | syslib::open::Flags::CLOSE_ON_EXEC))?;
        Ok(Self {
            socket,
            rx_msg: RingBuffer::new(1024),
            tx_msg: RingBuffer::new(1024),
            rx_fd: RingBuffer::new(8),
            tx_fd: RingBuffer::new(8)
        })
    }
    pub fn message(&mut self) -> Option<Result<Message, WlError<'static>>> {
        let req = self.rx_msg.get(1)?;
        let size = ((req & 0xFFFF_0000) >> 16) as u16;
        if size < 8 {
            return Some(Err(WlError::CORRUPT))
        }
        if self.rx_msg.len() < (size as usize) / std::mem::size_of::<u32>() {
            return None;
        }
        let opcode = (req & 0xFFFF) as u16;
        let object = Id(self.rx_msg.pop().unwrap());
        let _ = self.rx_msg.pop();
        Some(Ok(Message { object, opcode, size }))
    }
    pub fn send_message(&mut self, id: Id, opcode: u16) -> Result<CommitKey, WlError<'static>> {
        self.tx_msg.push(id.0);
        let key = CommitKey(self.tx_msg.front);
        if let Some(_) = self.tx_msg.push(opcode as u32) {
            Err(WlError::INTERNAL)
        } else {
            Ok(key)
        }
    }
    /// Commits a message, ammending the message header to include the pushed arguments.
    pub fn commit(&mut self, key: CommitKey) -> Result<(), WlError<'static>> {
        let len = if self.tx_msg.front > key.0 {
            self.tx_msg.front - key.0 + 1
        } else {
            (self.tx_msg.data.len() - key.0) + self.tx_msg.front + 1
        } * std::mem::size_of::<u32>();
        let req = self.tx_msg.get_linear_mut(key.0).ok_or(WlError::INTERNAL)?;
        *req = (*req & 0x0000_FFFF) | ((len as u32) << 16);
        Ok(())
    }
    pub fn i32(&mut self) -> Result<i32, WlError<'static>> {
        self.rx_msg.pop().map(|i| i as i32).ok_or(WlError::CORRUPT)
    }
    pub fn send_i32(&mut self, i32: i32) -> Result<(), WlError<'static>> {
        if let Some(_) = self.tx_msg.push(i32 as u32) {
            Err(WlError::INTERNAL)
        } else {
            Ok(())
        }
    }
    pub fn u32(&mut self) -> Result<u32, WlError<'static>> {
        self.rx_msg.pop().ok_or(WlError::CORRUPT)
    }
    pub fn send_u32(&mut self, u32: u32) -> Result<(), WlError<'static>> {
        if let Some(_) = self.tx_msg.push(u32) {
            Err(WlError::INTERNAL)
        } else {
            Ok(())
        }
    }
    pub fn fixed(&mut self) -> Result<Fixed, WlError<'static>> {
        self.rx_msg.pop().map(|i| Fixed::from_raw(i)).ok_or(WlError::CORRUPT)
    }
    pub fn send_fixed(&mut self, fixed: Fixed) -> Result<(), WlError<'static>> {
        if let Some(_) = self.tx_msg.push(fixed.0) {
            Err(WlError::INTERNAL)
        } else {
            Ok(())
        }
    }
    #[inline]
    pub fn string(&mut self) -> Result<String, WlError<'static>> {
        self.bytes().and_then(|bytes| String::from_utf8(bytes).map_err(|_| WlError::UTF_8))
    }
    #[inline]
    pub fn send_string(&mut self, string: &str) -> Result<(), WlError<'static>> {
        // Need to append null byte
        self.send_u32(0)?;
        Ok(())
    }
    pub fn object(&mut self) -> Result<Id, WlError<'static>> {
        self.rx_msg.pop().map(|i| Id(i)).ok_or(WlError::CORRUPT)
    }
    pub fn send_object(&mut self, object: Id) -> Result<(), WlError<'static>> {
        if let Some(_) = self.tx_msg.push(object.0) {
            Err(WlError::INTERNAL)
        } else {
            Ok(())
        }
    }
    pub fn new_id(&mut self) -> Result<NewId, WlError<'static>> {
        let interface = self.string()?.into();
        let version = self.u32()?;
        let id = self.object()?;
        Ok(NewId { id, version, interface })
    }
    pub fn bytes(&mut self) -> Result<Vec<u8>, WlError<'static>> {
        let len = self.u32()?;
        if len == 0 { return Ok(Vec::new()) }
        // divide by 4 rounding up
        let take_len = (len as usize >> 2) + (len & 0b11 != 0) as usize;
        if self.rx_msg.len() < take_len {
            return Err(WlError::CORRUPT)
        }
        let mut bytes: Vec<u8> = Vec::with_capacity(len as usize);
        use std::mem::size_of;
        if self.rx_msg.front > self.rx_msg.back {
            // Safety: The values in the range between `back` and `front` are initialised and any bit pattern is valid for u8
            unsafe {
                let src = self.rx_msg.data.as_ptr() as *const u8;
                bytes.as_mut_ptr().copy_from_nonoverlapping(src.add(self.rx_msg.back * size_of::<u32>()), len as usize);
                bytes.set_len(len as usize);
            }
        } else {
            // Safety: The values in the range between `back` and `front` are initialised and any bit pattern is valid for u8
            unsafe {
                let src = self.rx_msg.data.as_ptr() as *const u8;
                let part_len = self.rx_msg.data.len() * size_of::<u32>() - self.rx_msg.back * size_of::<u32>();
                bytes.as_mut_ptr().copy_from_nonoverlapping(src.add(self.rx_msg.back * size_of::<u32>()), part_len);
                bytes.as_mut_ptr().add(part_len).copy_from_nonoverlapping(src, self.rx_msg.front * size_of::<u32>());
                bytes.set_len(len as usize);
            }
        }
        self.rx_msg.back = (self.rx_msg.back + take_len) & (self.rx_msg.data.len() - 1);
        Ok(bytes)
    }
    pub fn send_bytes(&mut self, bytes: &[u8]) -> Result<(), WlError<'static>> {
        self.send_u32(0)?;
        let t = (self.rx_msg.front + self.rx_msg.data.len() - 1) & (self.rx_msg.data.len() - 1);
        if self.rx_msg.front == t {
            return Err(WlError::INTERNAL)
        }
        Ok(())
    }
    pub fn file(&mut self) -> Result<File, WlError<'static>> {
        self.rx_fd.pop().ok_or(WlError::CORRUPT)
    }
    pub fn send_file(&mut self, fd: Fd<'static>) -> Result<(), WlError<'static>> {
        if let Some(_) = self.tx_fd.push(fd) {
            Err(WlError::INTERNAL)
        } else {
            Ok(())
        }
    }

    /// Read from a file descriptor in to the buffer.
    /// 
    /// Returns true if any bytes were read. If the bytes read is not a multiple of `size_of::<u32>()`,
    /// the extra bytes are discarded.
    pub fn recvmsg(&mut self) -> crate::Result<bool> {
        use syslib::*;
        let t = (self.rx_msg.front + self.rx_msg.data.len() - 1) & (self.rx_msg.data.len() - 1);
        if self.rx_msg.front == t {
            return Ok(false)
        }
        let iov = unsafe {
            if self.rx_msg.front > t {
                [
                    IoVecMut::maybe_uninit(self.rx_msg.data.as_mut_ptr().add(self.rx_msg.front) as *mut u8, (self.rx_msg.data.len() - self.rx_msg.front) * std::mem::size_of::<u32>()),
                    IoVecMut::maybe_uninit(self.rx_msg.data.as_mut_ptr() as *mut u8, t * std::mem::size_of::<u32>())
                ]
            } else {
                [
                    IoVecMut::maybe_uninit(self.rx_msg.data.as_mut_ptr().add(self.rx_msg.front) as *mut u8, (t - self.rx_msg.front) * std::mem::size_of::<u32>()),
                    IoVecMut::maybe_uninit(std::ptr::null_mut(), 0)
                ]
            }
        };
        let mut ancillary = sock::Ancillary::<Fd, 8>::new();
        let read = syslib::recvmsg(&self.socket, &iov, Some(&mut ancillary), syslib::sock::Flags::NONE)? / std::mem::size_of::<u32>();
        self.rx_msg.front = (self.rx_msg.front + read) & (self.rx_msg.data.len() - 1);
        if ancillary.ty() == sock::AncillaryType::RIGHTS && ancillary.level() == sock::Level::SOCKET {
            for fd in ancillary.items() {
                // Safety: Fd is guaranteed to be valid for any bit-pattern and we trust the OS to return a valid fd when using SCM_RIGHTS
                self.rx_fd.push(unsafe { fd.assume_init().owned() });
            }
        }
        Ok(read != 0)
    }

    pub fn sendmsg(&mut self) -> crate::Result<()> {
        use syslib::*;
        use std::mem::size_of;
        let (iov, count) = unsafe {
            if self.tx_msg.front > self.tx_msg.back {
                let buf = &self.tx_msg.data[self.tx_msg.back..self.tx_msg.front];
                self.tx_msg.back = (self.tx_msg.back + buf.len()) & (self.tx_msg.data.len() - 1);
                (
                    [
                        IoVec::new(std::slice::from_raw_parts(buf.as_ptr() as *const u8, buf.len() * size_of::<u32>())),
                        IoVec::new(&[])
                    ],
                    1
                )
            } else if self.tx_msg.front == self.tx_msg.back {
                return Ok(());
            } else {
                let buf_back = &self.tx_msg.data[self.tx_msg.back..];
                let buf_front = &self.tx_msg.data[..self.tx_msg.front];
                (
                    [
                        IoVec::new(std::slice::from_raw_parts(buf_back.as_ptr() as *const u8, buf_back.len() * size_of::<u32>())),
                        IoVec::new(std::slice::from_raw_parts(buf_front.as_ptr() as *const u8, buf_front.len() * size_of::<u32>()))
                    ],
                    2
                )
            }
        };
        let mut ancillary = sock::Ancillary::<Fd, 8>::new();
        sendmsg(&self.socket, &iov[..count], Some(&ancillary), sock::Flags::NONE)?;
        let mut count = 8;
        loop {
            if let Some(item) = self.tx_fd.pop() {
                ancillary.add_item(item);
            } else {
                break
            }
            if count == 0 {
                break
            }
            count -= 1
        }
        Ok(())
    }
}

use std::mem::MaybeUninit;
/// A circular buffer suitable as a FIFO queue.
/// 
/// ```rust
/// use wl::wire::RingBuffer;
/// 
/// // Allocate a new buffer that can hold 4 elements
/// const ITEMS: &'static [&'static str] = &["apples", "oranges", "pears", "mangoes", "grapes", "bananas", "cherimoyas", "lemons"];
/// let mut buf = RingBuffer::new(ITEMS.len());
/// 
/// buf.push(ITEMS[0]);
/// 
/// for i in 1..ITEMS.len() {
///     buf.push(ITEMS[i]);
///     assert_eq!(buf.pop(), Some(ITEMS[i-1]));
/// }
/// ```
pub struct RingBuffer<T> {
    data: Box<[MaybeUninit<T>]>,
    front: usize,
    back: usize
}
impl<T> RingBuffer<T> {
    /// Create a new `RingBuffer` with the given size.
    /// 
    /// The maximum length is one less than the capacity as inserting to fill the buffer would cause
    /// an overflow.
    /// 
    /// ## Panics
    /// If `capacity` is not a multiple of 2.
    pub fn new(capacity: usize) -> Self {
        if !capacity.is_power_of_two() {
            panic!("Cannot construct a RingBuffer with a length of {capacity} as it is not a power of 2.")
        }
        let data = unsafe {
            let layout = std::alloc::Layout::array::<MaybeUninit<T>>(capacity).unwrap();
            let data = std::alloc::alloc(layout) as *mut MaybeUninit<T>;
            let slice = std::slice::from_raw_parts_mut(data, capacity);
            Box::from_raw(slice)
        };
        Self {
            data,
            front: 0,
            back: 0
        }
    }
    pub fn iter(&self) -> RingBufferIter<'_, T> {
        RingBufferIter { ring_buffer: self, index: 0 }
    }
    #[inline(always)]
    fn increment(&self, value: usize) -> usize {
        (value + 1) & (self.data.len() - 1)
    }
    /// Insert an element in to the `RingBuffer`, or return it back if there is no space.
    pub fn push(&mut self, value: T) -> Option<T> {
        let next = self.increment(self.front);
        if next == self.back {
            Some(value)
        } else {
            self.data[self.front] = MaybeUninit::new(value);
            self.front = next;
            None
        }
    }
    /// Remove the oldest item from the `RingBuffer` and return it.
    pub fn pop(&mut self) -> Option<T> {
        if self.front == self.back {
            None
        } else {
            let index = self.back;
            self.back = self.increment(self.back);
            Some(unsafe { self.data[index].assume_init_read() })
        }
    }
    /// Remove all items from the `RingBuffer`.
    pub fn clear(&mut self) {
        // For types with no special drop this would be as simples as setting front & back to 0.
        for s in self {
            drop(s)
        }
    }
    /// Get a reference to the item by index, where 0 is the oldest item.
    pub fn get(&self, index: usize) -> Option<&T> {
        let i = (self.back + index) & (self.data.len() - 1);
        if index < self.len() {
            Some(unsafe { self.data[i].assume_init_ref() })
        } else {
            None
        }
    }
    /// Get a mutable reference to the item by index, where 0 is the oldest item.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        let i = (self.back + index) & (self.data.len() - 1);
        if index < self.len() {
            Some(unsafe { self.data[i].assume_init_mut() })
        } else {
            None
        }
    }
    /// Get a reference by index relative to the underlying linear buffer.
    /// 
    /// Can be faster when you know the back pointer has not changed.
    pub fn get_linear(&self, index: usize) -> Option<&T> {
        if (self.front > self.back && index >= self.back && index < self.front) || (self.front < self.back && (index >= self.back || index < self.front)) {
            self.data.get(index).map(|t| unsafe { t.assume_init_ref()})
        } else {
            None
        }
    }
    /// Get a mutable reference by index relative to the underlying linear buffer.
    /// 
    /// Can be faster when you know the back pointer has not changed.
    pub fn get_linear_mut(&mut self, index: usize) -> Option<&mut T> {
        if (self.front > self.back && index >= self.back && index < self.front) || (self.front < self.back && (index >= self.back || index < self.front)) {
            self.data.get_mut(index).map(|t| unsafe { t.assume_init_mut()})
        } else {
            None
        }
    }
    /// Get the index of the front pointer
    pub fn front(&self) -> usize {
        self.front
    }
    /// Get the index of the back pointer
    pub fn back(&self) -> usize {
        self.back
    }
    /// Return the number of items in the `RingBuffer`.
    pub fn len(&self) -> usize {
        if self.front < self.back {
            (self.front + self.data.len()) - self.back
        } else {
            self.front - self.back
        }
    }
    /// Return the number of items that can be inserted before the buffer is full.
    pub fn free(&self) -> usize {
        self.data.len() - (
            if self.front < self.back {
                (self.front + self.data.len()) - self.back
            } else {
                self.front - self.back
            }
        )
    }
    /// Return the maximum number of items the RingBuffer` can hold.
    pub fn capacity(&self) -> usize {
        self.data.len()
    }
    /// Returns true if there are no items in the `RingBuffer`, or false otherwise.
    pub fn is_empty(&self) -> bool {
        self.front == self.back
    }
    /// Returns true if there is no more space to insert an item in to the `RingBuffer`, or false otherwise.
    pub fn is_full(&self) -> bool {
        self.front == self.back
    }
}
impl<T> Drop for RingBuffer<T> {
    fn drop(&mut self) {
        for value in self {
            std::mem::drop(value)
        }
    }
}
impl<T> Iterator for RingBuffer<T> {
    type Item = T;
    fn next(&mut self) -> Option<Self::Item> {
        self.pop()        
    }
}
impl<T: Clone + Copy> Clone for RingBuffer<T> {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
            front: self.front,
            back: self.back
        }
    }
}
impl<T: Debug> Debug for RingBuffer<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.iter())
            .finish()
    }
}

pub struct RingBufferIter<'a, T> {
    ring_buffer: &'a RingBuffer<T>,
    index: usize
}
impl<'a, T> Iterator for RingBufferIter<'a, T> {
    type Item = &'a T;
    fn next(&mut self) -> Option<Self::Item> {
        let index = self.index;
        self.index += 1;
        self.ring_buffer.get(index)
    }
}
pub struct RingBufferIterMut<'a, T> {
    ring_buffer: &'a mut RingBuffer<T>,
    index: usize
}
impl<'a, T> Iterator for RingBufferIterMut<'a, T> {
    type Item = &'a mut T;
    fn next(&mut self) -> Option<Self::Item> {
        let index = self.index;
        self.index += 1;
        self.ring_buffer.get_mut(index).map(|i| unsafe { &mut *(i as *mut T) })
    }
}