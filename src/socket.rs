use std::{path::Path, os::unix::prelude::{OsStrExt, FromRawFd}, collections::VecDeque, fs::File};
use libc::*;
use crate::RingBuffer;

fn prepare_socket<P: AsRef<Path>, F: Fn(i32, *const sockaddr, u32) -> std::io::Result<T>, T>(path: P, f: F) -> std::io::Result<T> {
    let path = path.as_ref().as_os_str().as_bytes();
    unsafe {
        let socket = socket(PF_LOCAL, SOCK_STREAM | SOCK_CLOEXEC, 0);
        let flags = fcntl(socket, F_GETFD);
        if fcntl(socket, F_SETFD, flags | FD_CLOEXEC) < 0 {
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
    socket: i32
}
impl UnixListener {
    pub fn bind<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        prepare_socket(path, |socket, address, len| unsafe {
            if 
                bind(socket, address, len) != 0 || 
                listen(socket, SOMAXCONN) != 0
            {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(Self {
                    socket
                })
            }
        })
    }
}
impl Iterator for UnixListener {
    type Item = UnixStream;
    fn next(&mut self) -> Option<Self::Item> {
        let socket = unsafe { accept(self.socket, std::ptr::null_mut(), std::ptr::null_mut()) };
        if socket < 0 {
            None
        } else {
            Some(UnixStream {
                socket,
                cmsg_buffer: [0; UnixStream::BUFFER_SIZE]
            })
        }
    }
}
pub struct UnixStream {
    socket: i32,
    cmsg_buffer: [u8; Self::BUFFER_SIZE]
}
impl UnixStream {
    const MAX_FD: usize = 8;
    const BUFFER_SIZE: usize = cmsg_space(Self::MAX_FD * std::mem::size_of::<i32>());
    pub fn connect<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        prepare_socket(path, |socket, address, len| unsafe {
            if connect(socket, address, len) != 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(Self {
                    socket,
                    cmsg_buffer: [0; Self::BUFFER_SIZE]
                })
            }
        })
    }
    pub fn poll(&self) {

    }
    /// Receive a message from the socket alongside ancillary data
    /// All file descriptors must be appropriately closed
    pub fn recvmsg(&mut self, buffer: &mut RingBuffer, fds: &mut VecDeque<File>) -> std::io::Result<bool> {
        use std::ptr::null_mut;
        use std::mem::size_of;
        let mut did_read = false;
        unsafe {
            loop {
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
                use std::io::{self, ErrorKind};
                match recvmsg(self.socket, &mut msg, MSG_DONTWAIT | MSG_CMSG_CLOEXEC) {
                    0 => break if did_read {
                        Ok(did_read)
                    } else {
                        Err(io::Error::new(ErrorKind::BrokenPipe, "Socket is closed"))
                    },
                    read if read > 0 => {
                        did_read = true;
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
                    },
                    _ => break match io::Error::last_os_error().kind() {
                        ErrorKind::WouldBlock => Ok(did_read),
                        _ if did_read => Ok(did_read),
                        kind => Err(io::Error::new(kind, "Unable to receive message from socket"))
                    }
                }
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
            if sendmsg(self.socket, &msg, MSG_NOSIGNAL) < 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        }

    }
}
impl Drop for UnixStream {
    fn drop(&mut self) {
        unsafe {
            close(self.socket);
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