use std::any::Any;

use wl::{server::prelude::*, wire::Stream};
use syslib::*;

// Testing dynamic dispatch and type erasure with Lease system

fn dispatch_a<T>(object: Lease<dyn Any>, event_loop: &mut EventLoop<T>, client: &mut Client<T>) -> std::result::Result<(), WlError> {
    let object: Lease<i32> = object.downcast().unwrap();
    println!("a: {:?}", *object);
    Ok(())
}
fn dispatch_b<T>(object: Lease<dyn Any>, event_loop: &mut EventLoop<T>, client: &mut Client<T>) -> std::result::Result<(), WlError> {
    let object: Lease<&'static str> = object.downcast().unwrap();
    println!("b: {:?}", *object);
    Ok(())
}

fn main() {

    let a = Resident::new(133.into(), dispatch_a::<()>, "apples", 6, 10);
    let b = Resident::new(42.into(), dispatch_b::<()>, "oranges", 7, "apples");

    let objs = vec![a.into_any(), b.into_any()];
    let stream = Stream::connect("/var/run/user/1000/wayland-0").unwrap();
    let mut client = Client::new(stream);
    for mut obj in objs {
        let mut event_loop = EventLoop::new(()).unwrap();
        let dispatch = obj.dispatch();
        dispatch(obj.lease().unwrap(), &mut event_loop, &mut client).unwrap();
    }
    // Prints:
    // a: 10
    // b: "apples"
}