use std::any::Any;

use wl::server::prelude::*;
use syslib::*;

pub struct Display;
fn wl_display_dispatch<T>(this: Lease<dyn Any>, event_loop: &mut EventLoop<T>, client: &mut Client<T>, message: Message) -> Result<(), WlError<'static>> {
    println!("got message on display object: {:?}", message);
    let id = client.stream().object()?;
    println!("registry_id: {:?}", id);
    let _ = client.remove(id)?;
    Ok(())
}

fn wl_init<T>(event_loop: &mut EventLoop<T>, client: &mut Client<T>, version: u32) -> Resident<T> {
    wl::lease::Resident::new(1.into(), wl_display_dispatch, "wl_display", version, Display).into_any()
}

fn main() {
    syslib::unlink("test.socket").unwrap();
    let mut event_loop = Server::event_loop((), "test.socket", wl_init).unwrap();
    let mut count = 0;
    loop {
        event_loop.wait(u32::MAX).unwrap();
        count += 1;
        if count > 30 { break }
    }
}