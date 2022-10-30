use std::{fmt::Debug, path::Path, ops::{Deref, DerefMut}};

use crate::prelude::*;
use ahash::{HashMap, HashMapExt};
use syslib::{Socket, File, FileDescriptor};

pub struct Interface {
    version: u32,
    interface: String
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Id(u32);
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
    interface: Option<Interface>,
    id: Id
}
impl NewId {
    #[inline]
    pub fn interface(&self) -> &Option<Interface> {
        &self.interface
    }
    #[inline]
    pub fn id(&self) -> Id {
        self.id
    }
}
/// Fixed decimal number as specified by the Wayland wire format
#[repr(transparent)]
pub struct Fixed(u32);

pub trait EventSource<T> {
    fn fd(&self) -> Fd<'static>;
    fn destroy(&mut self, event_loop: &mut EventLoop<T>) {}
    fn input(&mut self, event_loop: &mut EventLoop<T>) -> Result<()>;
}
pub struct EventLoop<T> {
    epoll: File,
    sources: HashMap<u32, Option<Box<dyn EventSource<T>>>>,
    pub state: T
}
impl<T> EventLoop<T> {
    pub fn new(state: T) -> Result<Self> {
        Ok(Self {
            epoll: syslib::epoll_create(syslib::epoll::Flags::CLOSE_ON_EXEC)?,
            sources: HashMap::new(),
            state
        })
    }
    pub fn add(&mut self, event_source: Box<dyn EventSource<T>>) -> Result<()> {
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
    pub fn wait(&mut self, timeout: u32) -> Result<()> {
        use syslib::epoll;
        let mut events: [MaybeUninit<epoll::Event>; 32] = std::array::from_fn(|_| std::mem::MaybeUninit::uninit());
        let events = syslib::epoll_wait(&self.epoll, &mut events, timeout)?;
        for event in events {
            let fd = unsafe { event.data.fd };
            let mut had_error = false;
            if event.events.any(epoll::Events::INPUT) {
                // Lease the event source so that it can modify its owning data structure
                let mut source = self.sources.get_mut(&fd.raw()).unwrap().take();
                had_error = source.as_mut().unwrap().input(self).is_err();
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
    pub fn listen<P: AsRef<Path>>(path: P) -> Result<Self> {
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
    pub rx_msg: RingBuffer<u32>,
    tx_msg: RingBuffer<u32>,
    rx_fd: RingBuffer<File>,
    tx_fd: RingBuffer<File>,
}
impl Stream {
    /// Open a new stream connected to a Unix domain socket.
    /// 
    /// `path` Must be less than 108 bytes long.
    pub fn connect<P: AsRef<Path>>(path: P) -> Result<Self> {
        use std::os::unix::prelude::OsStrExt;
        use syslib::sock::*;
        let socket = syslib::socket(Domain::UNIX, Type::STREAM | TypeFlags::CLOSE_ON_EXEC, Protocol::UNSPECIFIED)?;
        let address = UnixAddress::new(path.as_ref().as_os_str().as_bytes()).map_err(|_| Error::InvalidSocketPath)?;
        syslib::connect(&socket, address.address())?;

        Self::new(socket)
    }
    pub(crate) fn new(socket: Socket) -> Result<Self> {
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
    pub fn message(&mut self) -> Result<(Id, u16)> {
        todo!()
    }
    pub fn i32(&mut self) -> Result<i32> {
        todo!()
    }
    pub fn u32(&mut self) -> Result<i32> {
        todo!()
    }
    pub fn fixed(&mut self) -> Result<Fixed> {
        todo!()
    }
    pub fn string(&mut self) -> Result<String> {
        todo!()
    }
    pub fn object(&mut self) -> Result<Id> {
        todo!()
    }
    pub fn new_id(&mut self) -> Result<NewId> {
        todo!()
    }
    pub fn bytes(&mut self) -> Result<Vec<u8>> {
        todo!()
    }
    pub fn fd(&mut self) -> Result<Fd> {
        todo!()
    }

    /// Read from a file descriptor in to the buffer.
    /// 
    /// Returns true if any bytes were read. If the bytes read is not a multiple of `size_of::<u32>()`,
    /// the extra bytes are discarded.
    pub fn recvmsg(&mut self) -> Result<bool> {
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
        let mut ancillary = sock::Ancillary::<Fd, 4>::new();
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

    fn sendmsg() {
        todo!()
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
    /// Return the number of items in the `RingBuffer`.
    pub fn len(&self) -> usize {
        if self.front < self.back {
            (self.front + self.data.len()) - self.back
        } else {
            self.front - self.back
        }
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