use std::any::Any;
use wl::{server::prelude::*, Result};

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
    let id = client.new_id();
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
            Shm::UID => { client.insert(id, Shm)?; }
            Compositor::UID => { client.insert(id, Compositor)?; }
            Subcompositor::UID => { client.insert(id, Compositor)?; }
            WmBase::UID => { client.insert(id, Compositor)?; }
            _ => { display(client)?.error(client, self.object(), 1, format!("Unknown global {}", name))?; }
        }
        Ok(())
    }
}


struct Shm;
impl Global for Shm {
    const UID: u32 = 1;
}
#[protocol("wayland.toml")]
impl WlShm for Lease<Shm> {
    fn create_pool(&mut self, client: &mut Client, id: NewId, fd: Fd, size: i32) -> Result<()> {
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
        todo!()
    }
}
struct Surface;
#[protocol("wayland.toml")]
impl WlSurface for Lease<Surface> {
    fn destroy(&mut self, client: &mut Client) -> Result<()> {
        todo!()
    }
    fn attach(&mut self, client: &mut Client, buffer: u32, x: i32, y: i32) -> Result<()> {
        todo!()
    }
    fn damage(&mut self, client: &mut Client, x: i32, y: i32, width: i32, height: i32) -> Result<()> {
        todo!()
    }
    fn frame(&mut self, client: &mut Client, callback: NewId) -> Result<()> {
        todo!()
    }
    fn set_opaque_region(&mut self, client: &mut Client, region: u32) -> Result<()> {
        todo!()
    }
    fn set_input_region(&mut self, client: &mut Client, region: u32) -> Result<()> {
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
struct Subcompositor;
impl Global for Subcompositor {
    const UID: u32 = 3;
}
#[protocol("wayland.toml")]
impl WlSubcompositor for Lease<Subcompositor> {
    fn destroy(&mut self, client: &mut Client) -> Result<()> {
        todo!()
    }
    fn get_subsurface(&mut self, client: &mut Client, id: NewId, surface: u32, parent: u32) -> Result<()> {
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
        todo!()
    }
    fn create_positioner(&mut self, client: &mut Client, id: NewId) -> Result<()> {
        todo!()
    }
    fn get_xdg_surface(&mut self, client: &mut Client, id: NewId, surface: u32) -> Result<()> {
        todo!()
    }
    fn pong(&mut self, client: &mut Client, serial: u32) -> Result<()> {
        todo!()
    }
}