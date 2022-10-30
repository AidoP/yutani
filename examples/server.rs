use wl::server::prelude::*;
use syslib::*;

fn main() {
    let mut event_loop = Server::event_loop((), "test.socket", |_, _| todo!()).unwrap();
    loop {
        event_loop.wait(u32::MAX).unwrap()
    }
}