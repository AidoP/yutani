#![feature(unix_socket_ancillary_data)]
use std::{os::unix::{net::*, prelude::AsRawFd}, io::IoSlice, fs::File};

fn main() {
    let file = File::open("readme.md").unwrap();
    let mut stream = UnixStream::connect("/run/user/1000/wayland-0")
        .unwrap();
    let mut message = wl::Message::new(1, 0);
    message.push_fd(file.as_raw_fd());
    message.send(&mut stream).unwrap();
}