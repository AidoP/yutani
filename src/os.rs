use std::{io, path::Path, os::unix::prelude::{OsStrExt, FromRawFd}, ops::Deref, collections::{VecDeque, HashMap}, fs::File, rc::Rc, cell::RefCell};
use libc::*;
use crate::RingBuffer;

#[repr(transparent)]
pub struct Fd(c_int);
impl Fd {
    fn new(fd: c_int) -> io::Result<Self> {
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(fd))
        }
    }
}
impl Deref for Fd {
    type Target = c_int;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Drop for Fd {
    fn drop(&mut self) {
        unsafe {
            close(self.0);
        }
    }
}
#[repr(transparent)]
#[derive(PartialEq, Eq, Clone, Copy)]
pub struct Events(c_int);
impl Events {
    pub const NONE: Self = Self(0);
    pub const INPUT: Self = Self(EPOLLIN);
    pub const OUTPUT: Self = Self(EPOLLOUT);
    pub const HANGUP: Self = Self(EPOLLHUP);
    pub fn none(&self) -> bool {
        self.0 == 0
    }
    /// There is input available
    pub fn input(&self) -> bool {
        self.0 & EPOLLIN != 0
    }
    /// The event is ready for writing
    pub fn output(&self) -> bool {
        self.0 & EPOLLOUT != 0
    }
    /// The other end of the event source disconnected
    pub fn hangup(&self) -> bool {
        self.0 & EPOLLHUP != 0
    }
    pub fn raw(&self) -> c_int {
        self.0
    }
}
impl From<i32> for Events {
    fn from(i: i32) -> Self {
        Self(i)
    }
}
impl std::ops::BitAnd for Events {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}
impl std::ops::BitAndAssign for Events {
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 = self.0 & rhs.0
    }
}
impl std::ops::BitOr for Events {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}
impl std::ops::BitOrAssign for Events {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 = self.0 | rhs.0
    }
}
impl std::ops::BitXor for Events {
    type Output = Self;
    fn bitxor(self, rhs: Self) -> Self::Output {
        Self(self.0 ^ rhs.0)
    }
}
impl std::ops::BitXorAssign for Events {
    fn bitxor_assign(&mut self, rhs: Self) {
        self.0 = self.0 ^ rhs.0
    }
}
pub trait Event {
    fn fd(&self) -> &Fd;
    fn events(&self) -> Events;
    fn signal(&mut self, events: Events, event_listener: &mut EventListener);
}

/// Watches for events from various sources
pub struct EventListener {
    pollfd: Fd,
    events: HashMap<c_int, Option<Box<dyn Event>>>
}
impl EventListener {
    pub fn new() -> io::Result<Self> {
        let pollfd = Fd::new(unsafe { epoll_create1(EPOLL_CLOEXEC) })?;
        Ok(Self {
            pollfd,
            events: HashMap::new()
        })
    }
    pub fn register(&mut self, event: Box<dyn Event>) -> io::Result<()> {
        let fd = **event.fd();
        self.events.insert(fd, Some(event));
        let mut event_data = epoll_event {
            events: EPOLLIN as _,
            u64: fd as _
        };
        if 0 != unsafe { epoll_ctl(*self.pollfd, EPOLL_CTL_ADD, fd, &mut event_data) } {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
    /// Remove the event if it was registered with the event listener
    pub fn remove(&mut self, event: &dyn Event) {
        if let Some((fd, _)) = self.events.remove_entry(event.fd()) {
            unsafe { epoll_ctl(*self.pollfd, EPOLL_CTL_DEL, fd, std::ptr::null_mut()) };
        }
    }
    pub fn start(mut self) -> ! {
        let mut events = [epoll_event { events: 0, u64: 0 }; 8];
        loop {
            let ready = unsafe { epoll_wait(*self.pollfd, events.as_mut_ptr(), events.len() as _, -1) };
            for &epoll_event { events, u64: fd} in &events[..ready as usize] {
                let fd = fd as i32;
                let mut event = self.events.get_mut(&fd).map(|e| e.take()).flatten();
                if let Some(event) = &mut event {
                    event.signal((events as i32).into(), &mut self)
                }
                if let Some(empty_event) = self.events.get_mut(&fd) {
                    std::mem::swap(&mut event, empty_event)
                }
            }
        }
    }
}
#[derive(Debug, Clone, Copy)]
pub enum Clock {
    /// The time as experienced by the machine
    Monotonic,
    /// The time as shown by the user's setable clock
    Realtime,
    /// Like monotonic, but continues ticking while the machine is asleep
    Boottime,
    /// The same as realtime but the machine will wake if it is asleep
    /// 
    /// Requires CAP_WAKE_ALARM
    RealtimeAlarm,
    /// The same as boottime but the machine will wake if it is asleep
    /// 
    /// Requires CAP_WAKE_ALARM
    BoottimeAlarm
}
impl Clock {
    fn raw(&self) -> clockid_t {
        match self {
            Self::Monotonic => CLOCK_MONOTONIC,
            Self::Realtime => CLOCK_REALTIME,
            Self::Boottime => CLOCK_BOOTTIME,
            Self::RealtimeAlarm => CLOCK_REALTIME_ALARM,
            Self::BoottimeAlarm => CLOCK_BOOTTIME_ALARM,
        }
    }
}
pub struct Timer {
    timerfd: Fd,
    f: Option<Box<dyn FnMut(&mut Timer, u64)>>,
    once: bool,
    to_remove: bool
}
impl Timer {
    /// Create a new timer that is not set to execute at any time
    pub fn new<F: 'static + Fn(&mut Timer, u64)>(clock: Clock, f: F) -> io::Result<Self> {
        let timerfd = Fd::new(unsafe { timerfd_create(clock.raw(), TFD_CLOEXEC) })?;
        Ok(Self {
            timerfd,
            f: Some(Box::new(f)),
            once: true,
            to_remove: false
        })
    }
    pub fn once(&mut self, at: std::time::Duration) -> io::Result<()> {
        self.once = true;
        self.to_remove = false;
        let duration = timespec {
            tv_sec: at.as_secs() as _,
            tv_nsec: at.as_nanos() as _
        };
        let timer = itimerspec {
            it_interval: timespec {
                tv_sec: 0,
                tv_nsec: 0
            },
            it_value: duration
        };
        if 0 != unsafe { timerfd_settime(*self.timerfd, 0, &timer, std::ptr::null_mut()) } {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
    pub fn repeat(&mut self, duration: std::time::Duration) -> io::Result<()> {
        self.once = false;
        self.to_remove = false;
        let duration = timespec {
            tv_sec: duration.as_secs() as _,
            tv_nsec: duration.as_nanos() as _
        };
        let timer = itimerspec {
            it_interval: duration,
            it_value: duration
        };
        if 0 != unsafe { timerfd_settime(*self.timerfd, 0, &timer, std::ptr::null_mut()) } {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}
impl Event for Timer {
    fn fd(&self) -> &Fd {
        &self.timerfd
    }
    fn events(&self) -> Events {
        Events::INPUT
    }
    fn signal(&mut self, events: Events, event_listener: &mut EventListener) {
        if events.input() {
            self.to_remove = true;
            let mut expirations = 0u64;
            if 8 == unsafe { read(*self.timerfd, &mut expirations as *mut _ as _, std::mem::size_of_val(&expirations)) } {
                let mut f = self.f.take().unwrap();
                f(self, expirations);
                self.f = Some(f);
            }
        }
        if self.once && self.to_remove {
            event_listener.remove(self)
        }
    }
}

fn prepare_socket<P: AsRef<Path>, F: Fn(Fd, *const sockaddr, u32) -> std::io::Result<T>, T>(path: P, f: F) -> io::Result<T> {
    let path = path.as_ref().as_os_str().as_bytes();
    unsafe {
        let socket = Fd::new(socket(PF_LOCAL, SOCK_STREAM | SOCK_CLOEXEC, 0))?;
        let flags = fcntl(*socket, F_GETFD);
        if fcntl(*socket, F_SETFD, flags | FD_CLOEXEC) < 0 {
            return Err(std::io::Error::last_os_error())
        }
        let mut address = sockaddr_un {
            sun_family: AF_LOCAL as _,
            sun_path: [0; 108]
        };
        if path.len() > std::mem::size_of_val(&address.sun_path) {
            Err(std::io::Error::new(std::io::ErrorKind::Other, "Socket address path is too long"))
        } else {
            std::ptr::copy_nonoverlapping(path.as_ptr() as *const i8, address.sun_path.as_mut_ptr(), path.len());
            f(socket, &address as *const _ as _, std::mem::size_of::<sockaddr_un>() as _)
        }
    }
}
pub struct UnixListener {
    socket: Fd
}
impl UnixListener {
    pub fn bind<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        prepare_socket(path, |socket, address, len| unsafe {
            if 
                bind(*socket, address, len) != 0 || 
                listen(*socket, SOMAXCONN) != 0
            {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(Self {
                    socket
                })
            }
        })
    }
    pub fn on_accept<F: 'static + FnMut(UnixStream) -> Box<dyn Event>>(self, f: F) -> Box<dyn Event> {
        Box::new(UnixListenerEventSource {
            socket: self.socket,
            constructor: Box::new(f)
        })
    }
}
struct UnixListenerEventSource {
    socket: Fd,
    constructor: Box<dyn FnMut(UnixStream) -> Box<dyn Event>>
}
impl Event for UnixListenerEventSource {
    fn fd(&self) -> &Fd {
        &self.socket
    }
    fn events(&self) -> Events {
        Events::INPUT
    }
    fn signal(&mut self, _: Events, event_listener: &mut EventListener) {
        if let Ok(socket) = Fd::new(unsafe { accept(*self.socket, std::ptr::null_mut(), std::ptr::null_mut()) }) {
            let stream = UnixStream {
                socket,
                cmsg_buffer: [0; UnixStream::BUFFER_SIZE]
            };
            event_listener.register((self.constructor)(stream)).ok();
        }
    }
}
impl Iterator for UnixListener {
    type Item = UnixStream;
    fn next(&mut self) -> Option<Self::Item> {
        let socket = Fd::new(unsafe { accept(*self.socket, std::ptr::null_mut(), std::ptr::null_mut()) });
        socket.map(|socket| UnixStream {
            socket,
            cmsg_buffer: [0; UnixStream::BUFFER_SIZE]
        }).ok()
    }
}
pub struct UnixStream {
    socket: Fd,
    cmsg_buffer: [u8; Self::BUFFER_SIZE]
}
impl UnixStream {
    const MAX_FD: usize = 8;
    const BUFFER_SIZE: usize = cmsg_space(Self::MAX_FD * std::mem::size_of::<i32>());
    pub fn connect<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        prepare_socket(path, |socket, address, len| unsafe {
            if connect(*socket, address, len) != 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(Self {
                    socket,
                    cmsg_buffer: [0; Self::BUFFER_SIZE]
                })
            }
        })
    }
    pub fn fd(&self) -> &Fd {
        &self.socket
    }
    /// Receive a message from the socket alongside ancillary data
    /// All file descriptors must be appropriately closed
    pub fn recvmsg(&mut self, buffer: &mut RingBuffer, fds: &mut VecDeque<File>) -> io::Result<()> {
        use std::ptr::null_mut;
        use std::mem::size_of;
        unsafe {
            let mut iov = buffer.iov_mut();
            let mut msg = msghdr {
                msg_name: null_mut(),
                msg_namelen: 0,
                msg_iov: iov.as_mut_ptr() as _,
                msg_iovlen: iov.len(),
                msg_control: cmsg_align(self.cmsg_buffer.as_mut_ptr() as _) as _,
                msg_controllen: Self::BUFFER_SIZE,
                msg_flags: 0
            };
            let read = recvmsg(*self.socket, &mut msg, MSG_CMSG_CLOEXEC);
            if read <= 0 {
                Err(io::Error::last_os_error())
            } else {
                // read cannot be larger than the available space in iov
                buffer.add_writer(read as _);
                let mut cmsgp = CMSG_FIRSTHDR(&msg);
                while cmsgp != null_mut() {
                    if (*cmsgp).cmsg_type == SCM_RIGHTS && (*cmsgp).cmsg_level == SOL_SOCKET {
                        let count = ((*cmsgp).cmsg_len - CMSG_LEN(0) as usize) / size_of::<i32>();
                        let data = std::slice::from_raw_parts_mut(CMSG_DATA(cmsgp) as _, count);
                        for fd in data {
                            fds.push_back(File::from_raw_fd(*fd))
                        }
                    }
                    cmsgp = CMSG_NXTHDR(&msg, cmsgp);
                }
                Ok(())
            }
        }
    }
    /// Send a message from the socket alongside ancillary data
    pub fn sendmsg(&mut self, iov: &mut [IoVec], fds: &[i32]) -> std::io::Result<()> {
        use std::ptr::null_mut;
        use std::mem::size_of;
        let mut msg = msghdr {
            msg_name: null_mut(),
            msg_namelen: 0,
            msg_iov: iov.as_mut_ptr() as _,
            msg_iovlen: iov.len(),
            msg_control: cmsg_align(self.cmsg_buffer.as_mut_ptr() as _) as _,
            msg_controllen: cmsg_space(fds.len() * size_of::<i32>()),
            msg_flags: 0
        };
        // No one should be reaching this limit
        assert!(fds.len() <= Self::MAX_FD);
        unsafe {
            let cmsgp = CMSG_FIRSTHDR(&mut msg);
            (*cmsgp).cmsg_level = SOL_SOCKET;
            (*cmsgp).cmsg_type = SCM_RIGHTS;
            (*cmsgp).cmsg_len = CMSG_LEN((size_of::<i32>() * fds.len()) as _) as _;
            let mut dst = CMSG_DATA(cmsgp) as *mut i32;
            for fd in fds {
                *dst = *fd;
                dst = dst.add(1)
            }
            if sendmsg(*self.socket, &msg, MSG_NOSIGNAL) < 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        }
    }
}
const fn cmsg_align(len: usize) -> usize {
    use std::mem::size_of;
    (len + size_of::<usize>() - 1) & !(size_of::<usize>() - 1)
}
const fn cmsg_space(len: usize) -> usize {
    use std::mem::size_of;
    cmsg_align(len) + cmsg_align(size_of::<cmsghdr>())
}

#[repr(transparent)]
pub struct IoVec<'a>(iovec, std::marker::PhantomData<&'a u8>);
impl IoVec<'static> {
    pub fn empty() -> Self {
        Self(iovec { iov_base: std::ptr::null_mut(), iov_len: 0 }, Default::default())
    }
}
impl<'a> From<&'a mut [u8]> for IoVec<'a> {
    fn from(slice: &mut [u8]) -> Self {
        Self(iovec {
            iov_base: slice.as_mut_ptr() as _,
            iov_len: slice.len()
        }, Default::default())
    }
}