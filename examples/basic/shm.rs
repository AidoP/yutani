use std::{rc::Rc, fs::File, os::unix::prelude::AsRawFd};

use wl::server::prelude::*;
use crate::{Global, wayland};

pub struct WlShm;
impl Global for WlShm {
    const UID: u32 = 1;
}
impl wayland::WlShm for Lease<WlShm> {
    fn create_pool(&mut self, client: &mut Client, id: NewId, file: File, size: i32) -> Result<()> {
        client.insert(id, WlShmPool::new(file, size)?)?;
        Ok(())
    }
}
/// TODO: Handle SIGBUS to protect against the client resizing the buffer against our will
struct ShmMapping {
    memory: *mut u8,
    size: usize,
    file: File
}
impl ShmMapping {
    fn new(file: File, size: i32) -> Result<Self> {
        use libc::*;
        if size <= 0 {
            todo!()
        }
        let size = size as usize;
        let protection = PROT_READ | PROT_WRITE;
        let flags = MAP_SHARED;
        let memory = unsafe { mmap(std::ptr::null_mut(), size, protection, flags, file.as_raw_fd(), 0) };
        if memory == libc::MAP_FAILED {
            todo!()
        }
        Ok(Self {
            memory: memory as *mut u8,
            size,
            file
        })
    }
}
impl Drop for ShmMapping {
    fn drop(&mut self) {
        use libc::*;
        unsafe {
            munmap(self.memory as _, self.size);
            close(self.file.as_raw_fd());
        }
    }
}
/// A memory-mapped file allowing access to a shared memory between programs
pub struct WlShmPool {
    mapping: Rc<ShmMapping>
}
impl WlShmPool {
    fn new(file: File, size: i32) -> Result<Self> {
        Ok(Self {
            mapping: Rc::new(ShmMapping::new(file, size)?)
        })
    }
}
impl wayland::WlShmPool for Lease<WlShmPool> {
    fn destroy(&mut self, client: &mut Client) -> Result<()> {
        client.drop(self)
    }
    fn create_buffer(&mut self, client: &mut Client, id: NewId, offset: i32, width: i32, height: i32, stride: i32, format: u32) -> Result<()> {
        // Buffers require shared memory access (unsafe)
        // Also, how to drop the mmap once buffers are destroyed since the pool can be destroyed first
        todo!()
    }
    fn resize(&mut self, client: &mut Client, size: i32) -> Result<()> {
        if size <= 0 || size < self.mapping.size as i32 {
            todo!()
        }
        todo!()
    }
}
macro_rules! wl_formats {
    (ARGB8888, XRGB8888$(, $format:ident)*) => {
        pub enum Format {
            ARGB8888,
            XRGB8888,
            $($format),*
        }
        impl Format {
            pub fn new(format: u32) -> Result<Self> {
                match format {
                    wayland::WlShmFormat::ARGB8888 => Ok(Self::ARGB8888),
                    wayland::WlShmFormat::XRGB8888 => Ok(Self::XRGB8888),
                    $(WlShmEnumFormat::$format => Ok(Self::$format),)*
                    _ => todo!(/* User error system */)
                }
            }
            pub fn supported(client: &mut Client, mut shm: Lease<WlShm>) -> Result<()> {
                use wayland::WlShm;
                shm.format(client, wayland::WlShmFormat::ARGB8888)?;
                shm.format(client, wayland::WlShmFormat::XRGB8888)?;
                $(shm.format(client, wayland::WlShmFormat::$format)?;)*
                Ok(())
            }
        }
        impl Into<u32> for Format {
            fn into(self) -> u32 {
                match self {
                    Self::ARGB8888 => wayland::WlShmFormat::ARGB8888,
                    Self::XRGB8888 => wayland::WlShmFormat::XRGB8888,
                    $(Self::$format => wayland::WlShmFormat::$format),*
                }
            }
        }
    };
}
wl_formats!{ARGB8888, XRGB8888}

pub struct WlBuffer {
    mapping: Rc<ShmMapping>,
    buffer: *mut u8,
    width: usize,
    height: usize,
    stride: usize,
    format: Format
}
impl WlBuffer {
    fn new(mapping: Rc<ShmMapping>, offset: i32, width: i32, height: i32, stride: i32, format: u32) -> Result<Self> {
        let format = Format::new(format)?;
        if width <= 0 || height <= 0 || stride < 0 || offset < 0 {
            todo!()
        }
        let (width, height, stride, offset) = (width as usize, height as usize, stride as usize, offset as usize);
        if  stride < width || offset + stride * height >= mapping.size {
            todo!()
        }
        let buffer = unsafe { mapping.memory.add(offset) };
        Ok(Self {
            mapping,
            buffer,
            width,
            height,
            stride,
            format
        })
    }
    fn len(&self) -> usize {
        self.stride * self.height
    }
    fn get_mut(&mut self) -> &mut [u8] {
        // Safety: Violated due to shared memory access. This is unavoidable with a shared memory mapping.
        unsafe { std::slice::from_raw_parts_mut(self.buffer, self.len()) }
    }
}
impl wayland::WlBuffer for Lease<WlBuffer> {
    fn destroy(&mut self, client: &mut Client) -> Result<()> {
        client.drop(self)
    }
}