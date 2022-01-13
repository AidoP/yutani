use std::{fs::File, os::unix::prelude::AsRawFd};

fn main() {
    let file = File::open("readme.md").unwrap();
    let mut stream = wl::socket::UnixStream::connect("/run/user/1000/wayland-0")
        .unwrap();
    let mut message = wl::Message::new(1, 0);
    message.push_fd(wl::Fd::new(file.as_raw_fd()));
    message.send(&mut stream).unwrap();
}