use wl::server::prelude::*;

fn main() {
    let mut event_listener = EventListener::new().unwrap();
    let server = Server::listen(WlDisplay::default(), DisplayErrorHandler::default(), WlDisplay::drop_handler).unwrap();
    event_listener.register(server).unwrap();
}

#[derive(Default, Clone)]
struct DisplayErrorHandler;
impl DispatchErrorHandler for DisplayErrorHandler {
    fn handle(&mut self, client: &mut Client, error: wl::DispatchError) -> Result<()> {
        use wayland::WlDisplay;
        let mut display = display(client)?;
        match error {
            wl::DispatchError::ObjectNull => display.error(client, &0, wayland::WlDisplayError::INVALID_OBJECT, "Attempted to access the null object (id 0)"),
            wl::DispatchError::ObjectExists(object) => display.error(client, &object, wayland::WlDisplayError::INVALID_METHOD, "Cannot add the object as one with that id already exists"),
            wl::DispatchError::ObjectNotFound(object) => display.error(client, &object, wayland::WlDisplayError::INVALID_OBJECT, "The specified object does not exist"),
            wl::DispatchError::NoVariant { name, variant } => display.error(client, &Client::DISPLAY, wayland::WlDisplayError::INVALID_METHOD, &format!("Enum {:?} does not contain value {}", name, variant)),
            wl::DispatchError::InvalidRequest { opcode, object, interface } => display.error(client, &object, wayland::WlDisplayError::INVALID_METHOD, &format!("Interface {:?} has no request with opcode {}", interface, opcode)),
            wl::DispatchError::InvalidEvent { opcode, object, interface } => display.error(client, &object, wayland::WlDisplayError::INVALID_METHOD, &format!("Interface {:?} has no event with opcode {}", interface, opcode)),
            wl::DispatchError::UnexpectedObjectType { object, expected_interface, had_interface } => display.error(client, &object, wayland::WlDisplayError::INVALID_METHOD, &format!("Expected an object implementing {:?}, but got an object implementing {:?}", expected_interface, had_interface)),
            wl::DispatchError::ExpectedArgument { data_type } => display.error(client, &Client::DISPLAY, wayland::WlDisplayError::INVALID_METHOD, &format!("Method data corrupt, invalid data for a {:?}", data_type)),
            wl::DispatchError::Utf8Error(error) => display.error(client, &Client::DISPLAY, wayland::WlDisplayError::INVALID_METHOD, &format!("Only UTF-8 strings are supported - {:?}", error))
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
    type WlSeat = super::WlSeat;
    type WlPointer = super::WlPointer;
    type WlKeyboard = super::WlKeyboard;
    type WlTouch = super::WlTouch;
    type WlCompositor = super::WlCompositor;
    type WlSurface = super::WlSurface;
    type WlSubcompositor = super::WlSubcompositor;
    type WlSubsurface = super::WlSubsurface;
    type WlRegion = super::WlRegion;
    type WlOutput = super::WlOutput;
}
#[protocol("protocol/xdg-shell.toml")]
mod xdg_shell {
    use super::WlSurface as WlSurface;
    use super::WlSeat as WlSeat;
    use super::WlOutput as WlOutput;

    type XdgWmBase = super::XdgWmBase;
    type XdgSurface = super::XdgSurface;
    type XdgToplevel = super::XdgToplevel;
    type XdgPopup = super::XdgPopup;
    type XdgPositioner = super::XdgPositioner;
}

/// Lease out the display object
fn display(client: &mut Client) -> Result<Lease<WlDisplay>> {
    client.get(Client::DISPLAY)
}

#[derive(Default, Clone)]
pub struct WlDisplay {
    serial: u32
}
impl WlDisplay {
    pub fn serial(&mut self) -> u32 {
        self.serial = self.serial.wrapping_add(1);
        self.serial
    }
    fn drop_handler(client: &mut Client, object: Lease<dyn std::any::Any>) -> Result<()> {
        use wayland::WlDisplay;
        let mut display = display(client)?;
        display.delete_id(client, object.object())
    }
}
impl wayland::WlDisplay for Lease<WlDisplay> {
    fn sync(&mut self, client: &mut Client, callback: NewId) -> Result<()> {
        use wayland::WlCallback;
        let mut lease = client.insert(callback, WlCallback)?;
        lease.done(client, self.serial())?;
        client.delete(&lease)?;
        Ok(())
    }
    fn get_registry(&mut self, client: &mut Client, registry: NewId) -> Result<()> {
        let registry = &mut client.insert(registry, WlRegistry)?;
        global::<shm::WlShm>(registry, client)?;
        global::<WlCompositor>(registry, client)?;
        global::<WlSubcompositor>(registry, client)?;
        global::<XdgWmBase>(registry, client)?;
        Ok(())
    }
}
pub struct WlCallback;
impl wayland::WlCallback for Lease<WlCallback> {}
pub struct WlRegistry;
impl wayland::WlRegistry for Lease<WlRegistry> {
    fn bind(&mut self, client: &mut Client, global: u32, id: NewId) -> Result<()> {
        match global {
            shm::WlShm::UID => {
                let shm = client.insert(id, shm::WlShm)?;
                shm::Format::supported(client, shm)?;
            },
            WlCompositor::UID => {
                client.insert(id, WlCompositor)?;
            },
            WlSubcompositor::UID => {
                client.insert(id, WlSubcompositor)?;
            },
            XdgWmBase::UID => {
                client.insert(id, XdgWmBase)?;
            }
            _ => todo!()
        }
        Ok(())
    }
}


pub struct WlSeat;
impl wayland::WlSeat for Lease<WlSeat> {
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
pub struct WlPointer;
impl wayland::WlPointer for Lease<WlPointer> {
    fn release(&mut self, client: &mut Client) -> Result<()> {
        todo!()
    }
    fn set_cursor(&mut self, client: &mut Client, serial:u32, surface: Nullable<Lease<WlSurface>>, hotspot_x: i32, hotspot_y: i32) -> Result<()>  {
        todo!()
    }
    fn axis_source(&mut self, client: &mut Client, axis_source:u32) -> Result<()> {
        todo!()
    }
    fn axis_stop(&mut self, client: &mut Client, time:u32, axis:u32) -> Result<()> {
        todo!()
    }
}
pub struct WlKeyboard;
impl wayland::WlKeyboard for Lease<WlKeyboard> {
    fn release(&mut self, client: &mut Client) -> Result<()> {
        todo!()
    }
}
pub struct WlTouch;
impl wayland::WlPointer for Lease<WlTouch> {
    fn release(&mut self, client: &mut Client) -> Result<()> {
        todo!()
    }
    fn set_cursor(&mut self, client: &mut Client, serial:u32, surface: Nullable<Lease<WlSurface>>, hotspot_x: i32, hotspot_y: i32) -> Result<()>  {
        todo!()
    }
    fn axis_source(&mut self, client: &mut Client, axis_source:u32) -> Result<()> {
        todo!()
    }
    fn axis_stop(&mut self, client: &mut Client, time:u32, axis:u32) -> Result<()> {
        todo!()
    }
}
pub struct WlCompositor;
impl Global for WlCompositor {
    const UID: u32 = 2;
}
impl wayland::WlCompositor for Lease<WlCompositor> {
    fn create_surface(&mut self, client: &mut Client, id: NewId) -> Result<()> {
        client.insert(id, WlSurface)?;
        Ok(())
    }
    fn create_region(&mut self, client: &mut Client, id: NewId) -> Result<()> {
        client.insert(id, WlRegion)?;
        Ok(())
    }
}
pub struct WlSurface;
impl wayland::WlSurface for Lease<WlSurface> {
    fn destroy(&mut self, client: &mut Client) -> Result<()> {
        client.delete(self)
    }
    fn attach(&mut self, client: &mut Client, buffer: Nullable<Lease<shm::WlBuffer>>, x: i32, y: i32) -> Result<()> {
        todo!()
    }
    fn damage(&mut self, client: &mut Client, x: i32, y: i32, width: i32, height: i32) -> Result<()> {
        todo!()
    }
    fn frame(&mut self, client: &mut Client, callback: NewId) -> Result<()> {
        todo!()
    }
    fn set_opaque_region(&mut self, client: &mut Client, region: Nullable<Lease<WlRegion>>) -> Result<()> {
        todo!()
    }
    fn set_input_region(&mut self, client: &mut Client, region: Nullable<Lease<WlRegion>>) -> Result<()> {
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
    fn offset(&mut self, client: &mut Client, x: i32, y: i32) -> Result<()> {
        todo!()
    }
    fn commit(&mut self, client: &mut Client) -> Result<()> {
        todo!()
    }
}
pub struct WlRegion;
impl wayland::WlRegion for Lease<WlRegion> {
    fn destroy(&mut self, client: &mut Client) -> Result<()> {
        client.delete(self)
    }
    fn add(&mut self, client: &mut Client, x: i32, y: i32, width: i32, height: i32) -> Result<()> {
        todo!()
    }
    fn subtract(&mut self, client: &mut Client, x: i32, y: i32, width: i32, height: i32) -> Result<()> {
        todo!()
    }
}
pub struct WlOutput;
impl wayland::WlOutput for Lease<WlOutput> {
    fn release(&mut self, client: &mut Client) -> Result<()> {
        todo!()
    }
}
pub struct WlSubcompositor;
impl Global for WlSubcompositor {
    const UID: u32 = 3;
}
impl wayland::WlSubcompositor for Lease<WlSubcompositor> {
    fn destroy(&mut self, client: &mut Client) -> Result<()> {
        client.delete(self)
    }
    fn get_subsurface(&mut self, client: &mut Client, id: NewId, surface: Lease<WlSurface>, parent: Lease<WlSurface>) -> Result<()> {
        client.insert(id, WlSubsurface { surface: surface.object(), parent: parent.object() })?;
        Ok(())
    }
}
pub struct WlSubsurface {
    surface: u32,
    parent: u32
}
impl wayland::WlSubsurface for Lease<WlSubsurface> {
    fn destroy(&mut self, client: &mut Client) -> Result<()> {
        client.delete(self)
    }
    fn set_position(&mut self, client: &mut Client, x: i32, y: i32) -> Result<()> {
        todo!()
    }
    fn place_above(&mut self, client: &mut Client, sibling: Lease<WlSurface>) -> Result<()> {
        todo!()
    }
    fn place_below(&mut self, client: &mut Client, sibling: Lease<WlSurface>) -> Result<()> {
        todo!()
    }
    fn set_sync(&mut self, client: &mut Client) -> Result<()> {
        todo!()
    }
    fn set_desync(&mut self, client: &mut Client) -> Result<()> {
        todo!()
    }
}

pub struct XdgWmBase;
impl Global for XdgWmBase {
    const UID: u32 = 4;
}
impl xdg_shell::XdgWmBase for Lease<XdgWmBase> {
    fn destroy(&mut self, client: &mut Client) -> Result<()> {
        client.delete(self)
    }
    fn create_positioner(&mut self, client: &mut Client, id: NewId) -> Result<()> {
        todo!()
    }
    fn get_xdg_surface(&mut self, client: &mut Client, id: NewId, surface: Lease<WlSurface>) -> Result<()> {
        client.insert(id, XdgSurface { surface: surface.object() })?;
        Ok(())
    }
    fn pong(&mut self, client: &mut Client, serial: u32) -> Result<()> {
        todo!()
    }
}
pub struct XdgSurface {
    surface: u32
}
impl xdg_shell::XdgSurface for Lease<XdgSurface> {
    fn destroy(&mut self, client: &mut Client) -> Result<()> {
        client.delete(self)
    }
    fn get_toplevel(&mut self, client: &mut Client, id: NewId) -> Result<()> {
        client.insert(id, XdgToplevel::new(self))?;
        Ok(())
    }
    fn get_popup(&mut self, client: &mut Client, id: NewId, parent: Nullable<Lease<XdgSurface>>, positioner: Lease<XdgPositioner>) -> Result<()> {
        todo!()
    }
    fn set_window_geometry(&mut self, client: &mut Client, x: i32, y: i32, width: i32, height: i32) -> Result<()> {
        todo!()
    }
    fn ack_configure(&mut self, client: &mut Client, serial: u32) -> Result<()> {
        todo!()
    }
}
pub struct XdgToplevel {
    xdg_surface: u32,
    title: String,
    app_id: String
}
impl XdgToplevel {
    fn new(xdg_surface: &dyn Object) -> Self {
        Self {
            xdg_surface: xdg_surface.object(),
            title: Default::default(),
            app_id: Default::default()
        }
    }
}
impl xdg_shell::XdgToplevel for Lease<XdgToplevel> {
    fn destroy(&mut self, client: &mut Client) -> Result<()> {
        client.delete(self)
    }
    fn set_parent(&mut self, client: &mut Client, parent: Nullable<Lease<XdgToplevel>>) -> Result<()> {
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
    fn show_window_menu(&mut self, client: &mut Client, seat: Lease<WlSeat>, serial: u32, x: i32, y: i32) -> Result<()> {
        todo!()
    }
    fn r#move(&mut self, client: &mut Client, seat: Lease<WlSeat>, serial: u32) -> Result<()> {
        todo!()
    }
    fn resize(&mut self, client: &mut Client, seat: Lease<WlSeat>, serial: u32, edges: u32) -> Result<()> {
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
    fn set_fullscreen(&mut self, client: &mut Client, output: Nullable<Lease<WlOutput>>) -> Result<()> {
        todo!()
    }
    fn unset_fullscreen(&mut self, client: &mut Client) -> Result<()> {
        todo!()
    }
    fn set_minimized(&mut self, client: &mut Client) -> Result<()> {
        todo!()
    }
}
pub struct XdgPopup;
impl xdg_shell::XdgPopup for Lease<XdgPopup> {
    fn destroy(&mut self, client: &mut Client) -> Result<()> {
        client.delete(self)
    }
    fn grab(&mut self, client: &mut Client, seat: Lease<WlSeat> , serial:u32) -> Result<()>  {
        todo!()
    }
    fn reposition(&mut self, client: &mut Client, positioner: Lease<XdgPositioner> , token:u32) -> Result<()>  {
        todo!()
    }
}
pub struct XdgPositioner {
    xdg_surface: u32,
    title: String,
    app_id: String
}
impl xdg_shell::XdgPositioner for Lease<XdgPositioner> {
    fn destroy(&mut self, client: &mut Client) -> Result<()> {
        client.delete(self)
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
}