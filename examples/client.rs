#![feature(unix_socket_ancillary_data)]
use std::{os::unix::{net::*, prelude::AsRawFd}, io::IoSlice, fs::File};

fn main() {
    let file = File::open("readme.md").unwrap();
    let socket = UnixStream::connect("/run/user/1000/wayland-0")
        .unwrap();

    let mut buf = [0; 128];
    let mut anc = SocketAncillary::new(&mut buf);
    anc.add_fds(&[file.as_raw_fd()]);

    let buf = [01i32, 0x00080000];
    let buf: [u8; 2 * 4] = unsafe { std::mem::transmute(buf) };
    socket.send_vectored_with_ancillary(
        &[IoSlice::new(&buf)], 
        &mut anc
    ).unwrap();
}