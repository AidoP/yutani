use wl::{server::{self, Lease}, NewId};

/// The Protocol enum declares all the interfaces to generate dispatch glue for
/// The variant name is the interface to implement in CamelCase and each variant must have a single unnamed field specifying the object type to be stored
///
/// It is important to note that objects are stored in a hash map of this enum rather than of boxed traits. This is what allows for concrete type data to be preserved!
/// The concequence of this however is that all objects are the same size, the same size as the largest possible object type.
/// If you have large objects you should consider boxing them to reduce the memory overhead for smaller objects.
///
/// One of the interfaces must be tagged with `#[display]` and the object type must implement Default. This is to allow bootstrapping the protocol using the display singleton.
///
/// Interface specifications are stored in `./protocol/protocol_name.toml`. [Why use toml](https://github.com/AidoP/wl#why_toml)?
#[server::protocol("wayland")]
pub enum Protocol {
    #[display]
    WlDisplay(Display),
    WlRegistry(Registry),
    WlCallback(Callback)
}
type Client = server::Client<Protocol>;

#[derive(Default)]
pub struct Display {
    event_serial: u32
}
/// Interfaces are implemented on leases which include an object id on top of the inner object type.
/// The lease API is intended to reduce the chance of errors involving object id handling by introducing static typing to an otherwise polymorphic set of objects.
impl WlDisplay for Lease<Display> {
    fn sync(mut self, client: &mut Client, callback: NewId) -> Option<Self> {
        // Currently Client::upgrade is unsafe as it creates a lease which would cause API UB
        // This choice is under review as there is no risk of any memory unsafety
        unsafe { client.upgrade(callback, Callback) }.done(client, self.event_serial);
        self.event_serial = self.event_serial.wrapping_add(1);
        Some(self)
    }
    fn get_registry(self, client: &mut Client, registry: NewId) -> Option<Self> {
        let registry = client.reserve(registry, Registry).unwrap();
        
        // This example only shows the bare minimum necessary
        // All other interfaces that you support must be expressed here
        // Keep in mind some interfaces are implied to be supported when including others, eg, wl_shm_pool and wl_shm. Only wl_shm should be sent as a global.
        
        // registry.global(client, 0, "wl_shm", 1);
        
        Some(self)
    }
}

pub struct Registry;
impl WlRegistry for Lease<Registry> {
    fn bind(self, client: &mut Client, name: u32, id: NewId) -> Option<Self> {
        // The client has asked to create an object for one of the interfaces that you specified support for
        Some(self)
    }
}

pub struct Callback;
impl WlCallback for Lease<Callback> {}