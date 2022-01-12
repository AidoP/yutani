use wl::{server::prelude::*, Result};

fn main() {
    let server = wl::Server::bind().unwrap();
    server.start::<WlDisplay>()
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