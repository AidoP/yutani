use wl::server::prelude::*;

fn main() {
    let server = wl::Server::bind().unwrap();
    server.start::<WlDisplay, DisplayErrorHandler>()
}

#[derive(Default)]
struct DisplayErrorHandler;
impl DispatchErrorHandler for DisplayErrorHandler {
    fn handle(&mut self, client: &mut Client, error: wl::DispatchError) -> Result<()> {
        match error {
            wl::DispatchError::ObjectNull => todo!(),
            wl::DispatchError::ObjectExists(_) => todo!(),
            wl::DispatchError::ObjectNotFound(_) => todo!(),
            wl::DispatchError::NoVariant { name, variant } => todo!(),
            wl::DispatchError::InvalidRequest { opcode, object, interface } => todo!(),
            wl::DispatchError::InvalidEvent { opcode, object, interface } => todo!(),
            wl::DispatchError::UnexpectedObjectType { object, expected_interface, had_interface } => todo!(),
            wl::DispatchError::ExpectedArgument { data_type } => todo!(),
            wl::DispatchError::Utf8Error(_) => todo!(),
        }
    }
}

trait Global {
    const UID: u32;
}
fn global<T: Dispatch + Global>(registry: &mut Lease<WlRegistry>, client: &mut Client) -> Result<()> {
    use wayland::WlRegistry;
    registry.global(client, T::UID, T::INTERFACE, T::VERSION)
}

/// Shared Memory
mod shm;

#[protocol("protocol/wayland.toml")]
mod wayland {
    type WlDisplay = super::WlDisplay;
    type WlCallback = super::WlCallback;
    type WlRegistry = super::WlRegistry;
    type WlShm = super::shm::WlShm;
    type WlShmPool = super::shm::WlShmPool;
    type WlBuffer = super::shm::WlBuffer;
}

#[derive(Default)]
struct WlDisplay;
impl wayland::WlDisplay for Lease<WlDisplay> {
    fn sync(&mut self, client: &mut Client, callback: NewId) -> Result<()> {
        Ok(())
    }
    fn get_registry(&mut self, client: &mut Client, registry: NewId) -> Result<()> {
        let registry = &mut client.insert(registry, WlRegistry)?;
        global::<shm::WlShm>(registry, client)?;
        Ok(())
    }
}
struct WlCallback;
impl wayland::WlCallback for Lease<WlCallback> {
}
struct WlRegistry;
impl wayland::WlRegistry for Lease<WlRegistry> {
    fn bind(&mut self, client: &mut Client, global: u32, id: NewId) -> Result<()> {
        match global {
            shm::WlShm::UID => {
                let shm = client.insert(id, shm::WlShm)?;
                shm::Format::supported(client, shm)?;
            },
            _ => todo!()
        }
        Ok(())
    }
}

/*
use std::{any::Any, rc::Rc};
use wl::{server::prelude::*, Result, DispatchError};

fn main() {
    let server = wl::Server::bind().unwrap();
    server.start::<Display>()
}

/// Convenience function to avoid type inference issues
fn display(client: &mut Client) -> Result<Lease<Display>> {
    client.get(Client::DISPLAY)
}

trait Global {
    const UID: u32;
}
fn global<T: Dispatch + Global>(registry: &mut Lease<Registry>, client: &mut Client) -> Result<()> {
    registry.global(client, T::UID, T::INTERFACE.to_owned(), T::VERSION)
}

#[derive(Default)]
struct Display {
    serial: u32
}
impl Display {
    /// Get an auto-incrementing, wrapping serial number
    fn serial(&mut self) -> u32 {
        let serial = self.serial;
        self.serial = self.serial.wrapping_add(1);
        serial
    }
}
#[protocol("wayland.toml")]
impl WlDisplay for Lease<Display> {
    fn sync(&mut self, client: &mut Client, id: NewId) -> Result<()> {
        let mut callback = client.temporary(id, Callback)?;
        callback.done(client, self.serial())?;
        self.delete_id(client, callback.object())?;
        Ok(())
    }
    fn get_registry(&mut self, client: &mut Client, id: NewId) -> Result<()> {
        let registry = &mut client.insert(id, Registry)?;
        global::<Shm>(registry, client)?;
        global::<Compositor>(registry, client)?;
        global::<Subcompositor>(registry, client)?;
        global::<WmBase>(registry, client)?;
        Ok(())
    }
}

struct Callback;
#[protocol("wayland.toml")]
impl WlCallback for Lease<Callback> {}

struct Registry;
#[protocol("wayland.toml")]
impl WlRegistry for Lease<Registry> {
    fn bind(&mut self, client: &mut Client, name: u32, id: NewId) -> Result<()> {
        match name {
            Shm::UID => { Shm::formats(&mut client.insert(id, Shm)?, client)?; }
            Compositor::UID => { client.insert(id, Compositor)?; }
            Subcompositor::UID => { client.insert(id, Subcompositor)?; }
            WmBase::UID => { client.insert(id, WmBase)?; }
            _ => { display(client)?.error(client, self, 1, format!("Unknown global {}", name))?; }
        }
        Ok(())
    }
}
/// Shared memory, a concept that will never be enjoyable in Rust
struct Shm;
impl Global for Shm {
    const UID: u32 = 1;
}
impl Shm {
    fn formats(shm: &mut Lease<Self>, client: &mut Client) -> Result<()> {
        Format::supported(client, shm)
    }
}
#[protocol("wayland.toml")]
impl WlShm for Lease<Shm> {
    fn create_pool(&mut self, client: &mut Client, id: NewId, fd: Fd, size: i32) -> Result<()> {
        client.insert(id, ShmPool::new(fd, size)?)?;
        Ok(())
    }
}
/// TODO: Handle SIGBUS to protect against the client resizing the buffer against our will
struct ShmMapping {
    memory: *mut u8,
    size: usize,
    fd: Fd
}
impl ShmMapping {
    fn new(fd: Fd, size: i32) -> Result<Self> {
        use libc::*;
        if size <= 0 {
            todo!()
        }
        let size = size as usize;
        let protection = PROT_READ | PROT_WRITE;
        let flags = MAP_SHARED;
        let memory = unsafe { mmap(std::ptr::null_mut(), size, protection, flags, *fd, 0) };
        if memory == libc::MAP_FAILED {
            todo!()
        }
        Ok(Self {
            memory: memory as *mut u8,
            size,
            fd
        })
    }
}
impl Drop for ShmMapping {
    fn drop(&mut self) {
        use libc::*;
        unsafe {
            munmap(self.memory as _, self.size);
            close(*self.fd);
        }
    }
}
/// A memory-mapped file allowing access to a shared memory between programs
struct ShmPool {
    mapping: Rc<ShmMapping>
}
impl ShmPool {
    fn new(fd: Fd, size: i32) -> Result<Self> {
        Ok(Self {
            mapping: Rc::new(ShmMapping::new(fd, size)?)
        })
    }
}
#[protocol("wayland.toml")]
impl WlShmPool for Lease<ShmPool> {
    fn destroy(&mut self, client: &mut Client) -> Result<()> {
        client.drop(self)
    }
    fn create_buffer(&mut self, client: &mut Client, id: NewId, offset: i32, width: i32, height: i32, stride: i32, format: u32) -> Result<()> {
        // Buffers require shared memory access (unsafe)
        // Also, how to drop the mmap once buffers are destroyed since the pool can be destroyed first
        todo!()
    }
    fn resize(&mut self, client: &mut Client, size: i32) -> Result<()> {
        if size <= 0 {
            todo!()
        }
        if size < self.mapping.size as i32 {
            let error = format!("Passed size, {}, is smaller than the existing buffer size of {}", size, self.mapping.size);
            display(client)?.error(client, self, 1, error)?;
        }
        todo!()
    }
}
macro_rules! wl_formats {
    (ARGB8888, XRGB8888$(, $format:ident)*) => {
        enum Format {
            ARGB8888,
            XRGB8888,
            $($format),*
        }
        impl Format {
            fn new(format: u32) -> Result<Self> {
                match format {
                    WlShmEnumFormat::ARGB8888 => Ok(Self::ARGB8888),
                    WlShmEnumFormat::XRGB8888 => Ok(Self::XRGB8888),
                    $(WlShmEnumFormat::$format => Ok(Self::$format),)*
                    _ => todo!(/* User error system */)
                }
            }
            fn supported(client: &mut Client, shm: &mut Lease<Shm>) -> Result<()> {
                shm.format(client, WlShmEnumFormat::ARGB8888)?;
                shm.format(client, WlShmEnumFormat::XRGB8888)?;
                $(shm.format(client, WlShmEnumFormat::$format)?;)*
                Ok(())
            }
        }
        impl Into<u32> for Format {
            fn into(self) -> u32 {
                match self {
                    Self::ARGB8888 => WlShmEnumFormat::ARGB8888,
                    Self::XRGB8888 => WlShmEnumFormat::XRGB8888,
                    $(Self::$format => WlShmEnumFormat::$format),*
                }
            }
        }
    };
}
wl_formats!{ARGB8888, XRGB8888}

struct Buffer {
    mapping: Rc<ShmMapping>,
    buffer: *mut u8,
    width: usize,
    height: usize,
    stride: usize,
    format: Format
}
impl Buffer {
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
#[protocol("wayland.toml")]
impl WlBuffer for Lease<Buffer> {
    fn destroy(&mut self, client: &mut Client) -> Result<()> {
        client.drop(self)
    }
}
struct Seat;
#[protocol("wayland.toml")]
impl WlSeat for Lease<Seat> {
    fn get_pointer(&mut self, client: &mut Client, id: NewId) -> Result<()> {
        todo!()
    }
    fn get_keyboard(&mut self, client: &mut Client, id: NewId) -> Result<()> {
        todo!()
    }
    fn get_touch(&mut self, client: &mut Client, id: NewId) -> Result<()> {
        todo!()
    }
    fn release(&mut self, client: &mut Client) -> Result<()> {
        todo!()
    }
}
struct Compositor;
impl Global for Compositor {
    const UID: u32 = 2;
}
#[protocol("wayland.toml")]
impl WlCompositor for Lease<Compositor> {
    fn create_surface(&mut self, client: &mut Client, id: NewId) -> Result<()> {
        client.insert(id, Surface)?;
        Ok(())
    }
    fn create_region(&mut self, client: &mut Client, id: NewId) -> Result<()> {
        client.insert(id, Region)?;
        Ok(())
    }
}
struct Surface;
#[protocol("wayland.toml")]
impl WlSurface for Lease<Surface> {
    fn destroy(&mut self, client: &mut Client) -> Result<()> {
        client.drop(self)
    }
    fn attach(&mut self, client: &mut Client, buffer: &mut dyn WlBuffer, x: i32, y: i32) -> Result<()> {
        todo!()
    }
    fn damage(&mut self, client: &mut Client, x: i32, y: i32, width: i32, height: i32) -> Result<()> {
        todo!()
    }
    fn frame(&mut self, client: &mut Client, callback: NewId) -> Result<()> {
        todo!()
    }
    fn set_opaque_region(&mut self, client: &mut Client, region: &mut dyn WlRegion) -> Result<()> {
        todo!()
    }
    fn set_input_region(&mut self, client: &mut Client, region: &mut dyn WlRegion) -> Result<()> {
        todo!()
    }
    fn set_buffer_transform(&mut self, client: &mut Client, transform: i32) -> Result<()> {
        todo!()
    }
    fn set_buffer_scale(&mut self, client: &mut Client, scale: i32) -> Result<()> {
        todo!()
    }
    fn damage_buffer(&mut self, client: &mut Client, x: i32, y: i32, width: i32, height: i32) -> Result<()> {
        todo!()
    }
    fn commit(&mut self, client: &mut Client) -> Result<()> {
        todo!()
    }
}
struct Region;
#[protocol("wayland.toml")]
impl WlRegion for Lease<Region> {
    fn destroy(&mut self, client: &mut Client) -> Result<()> {
        client.drop(self)
    }
    fn add(&mut self, client: &mut Client, x: i32, y: i32, width: i32, height: i32) -> Result<()> {
        todo!()
    }
    fn subtract(&mut self, client: &mut Client, x: i32, y: i32, width: i32, height: i32) -> Result<()> {
        todo!()
    }
}
struct Output;
#[protocol("wayland.toml")]
impl WlOutput for Lease<Output> {
    fn release(&mut self, client: &mut Client) -> Result<()> {
        todo!()
    }
}
struct Subcompositor;
impl Global for Subcompositor {
    const UID: u32 = 3;
}
#[protocol("wayland.toml")]
impl WlSubcompositor for Lease<Subcompositor> {
    fn destroy(&mut self, client: &mut Client) -> Result<()> {
        client.drop(self)
    }
    fn get_subsurface(&mut self, client: &mut Client, id: NewId, surface: &mut dyn WlSurface, parent: &mut dyn WlSurface) -> Result<()> {
        client.insert(id, Subsurface { surface: surface.object(), parent: parent.object() })?;
        Ok(())
    }
}
struct Subsurface {
    surface: u32,
    parent: u32
}
impl Global for Subsurface {
    const UID: u32 = 3;
}
#[protocol("wayland.toml")]
impl WlSubsurface for Lease<Subsurface> {
    fn destroy(&mut self, client: &mut Client) -> Result<()> {
        client.drop(self)
    }
    fn set_position(&mut self, client: &mut Client, x: i32, y: i32) -> Result<()> {
        todo!()
    }
    fn place_above(&mut self, client: &mut Client, sibling: &mut dyn WlSurface) -> Result<()> {
        todo!()
    }
    fn place_below(&mut self, client: &mut Client, sibling: &mut dyn WlSurface) -> Result<()> {
        todo!()
    }
    fn set_sync(&mut self, client: &mut Client) -> Result<()> {
        todo!()
    }
    fn set_desync(&mut self, client: &mut Client) -> Result<()> {
        todo!()
    }
}

pub struct WmBase;
impl Global for WmBase {
    const UID: u32 = 4;
}
#[protocol("xdg-shell.toml")]
impl XdgWmBase for Lease<WmBase> {
    fn destroy(&mut self, client: &mut Client) -> Result<()> {
        client.drop(self)
    }
    fn create_positioner(&mut self, client: &mut Client, id: NewId) -> Result<()> {
        todo!()
    }
    fn get_xdg_surface(&mut self, client: &mut Client, id: NewId, surface: &mut dyn WlSurface) -> Result<()> {
        client.insert(id, SurfaceXdg { surface: surface.object() })?;
        Ok(())
    }
    fn pong(&mut self, client: &mut Client, serial: u32) -> Result<()> {
        todo!()
    }
}
pub struct SurfaceXdg {
    surface: u32
}
#[protocol("xdg-shell.toml")]
impl XdgSurface for Lease<SurfaceXdg> {
    fn destroy(&mut self, client: &mut Client) -> Result<()> {
        client.drop(self)
    }
    fn get_toplevel(&mut self, client: &mut Client, id: NewId) -> Result<()> {
        client.insert(id, Toplevel::new(self))?;
        Ok(())
    }
    fn get_popup(&mut self, client: &mut Client, id: NewId, parent: &mut dyn XdgSurface, positioner: &mut dyn XdgPositioner) -> Result<()> {
        todo!()
    }
    fn set_window_geometry(&mut self, client: &mut Client, x: i32, y: i32, width: i32, height: i32) -> Result<()> {
        todo!()
    }
    fn ack_configure(&mut self, client: &mut Client, serial: u32) -> Result<()> {
        todo!()
    }
}
pub struct Toplevel {
    xdg_surface: u32,
    title: String,
    app_id: String
}
impl Toplevel {
    fn new(xdg_surface: &dyn Object) -> Self {
        Self {
            xdg_surface: xdg_surface.object(),
            title: Default::default(),
            app_id: Default::default()
        }
    }
}
#[protocol("xdg-shell.toml")]
impl XdgToplevel for Lease<Toplevel> {
    fn destroy(&mut self, client: &mut Client) -> Result<()> {
        client.drop(self)
    }
    fn set_parent(&mut self, client: &mut Client, parent: &mut dyn XdgToplevel) -> Result<()> {
        todo!()
    }
    fn set_title(&mut self, client: &mut Client, title: String) -> Result<()> {
        self.title = title;
        Ok(())
    }
    fn set_app_id(&mut self, client: &mut Client, app_id: String) -> Result<()> {
        self.app_id = app_id;
        Ok(())
    }
    fn show_window_menu(&mut self, client: &mut Client, seat: &mut dyn WlSeat, serial: u32, x: i32, y: i32) -> Result<()> {
        todo!()
    }
    fn r#move(&mut self, client: &mut Client, seat: &mut dyn WlSeat, serial: u32) -> Result<()> {
        todo!()
    }
    fn resize(&mut self, client: &mut Client, seat: &mut dyn WlSeat, serial: u32, edges: u32) -> Result<()> {
        todo!()
    }
    fn set_max_size(&mut self, client: &mut Client, width: i32, height: i32) -> Result<()> {
        todo!()
    }
    fn set_min_size(&mut self, client: &mut Client, width: i32, height: i32) -> Result<()> {
        todo!()
    }
    fn set_maximized(&mut self, client: &mut Client) -> Result<()> {
        todo!()
    }
    fn unset_maximized(&mut self, client: &mut Client) -> Result<()> {
        todo!()
    }
    fn set_fullscreen(&mut self, client: &mut Client, output: &mut dyn WlOutput) -> Result<()> {
        todo!()
    }
    fn unset_fullscreen(&mut self, client: &mut Client) -> Result<()> {
        todo!()
    }
    fn set_minimized(&mut self, client: &mut Client) -> Result<()> {
        todo!()
    }
}
pub struct Positioner {
    xdg_surface: u32,
    title: String,
    app_id: String
}
#[protocol("xdg-shell.toml")]
impl XdgPositioner for Lease<Positioner> {
    fn destroy(&mut self, client: &mut Client) -> Result<()> {
        client.drop(self)
    }
    fn set_size(&mut self, client: &mut Client, width: i32, height: i32) -> Result<()> {
        todo!()
    }
    fn set_anchor_rect(&mut self, client: &mut Client, x: i32, y: i32, width: i32, height: i32) -> Result<()> {
        todo!()
    }
    fn set_anchor(&mut self, client: &mut Client, anchor: u32) -> Result<()> {
        todo!()
    }
    fn set_gravity(&mut self, client: &mut Client, gravity: u32) -> Result<()> {
        todo!()
    }
    fn set_constraint_adjustment(&mut self, client: &mut Client, constraint_adjustment: u32) -> Result<()> {
        todo!()
    }
    fn set_offset(&mut self, client: &mut Client, x: i32, y: i32) -> Result<()> {
        todo!()
    }
    fn set_reactive(&mut self, client: &mut Client) -> Result<()> {
        todo!()
    }
    fn set_parent_size(&mut self, client: &mut Client, width: i32, height: i32) -> Result<()> {
        todo!()
    }
    fn set_parent_configure(&mut self, client: &mut Client, serial: u32) -> Result<()> {
        todo!()
    }
}*/