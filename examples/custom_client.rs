use std::fs::File;

fn main() {
    let file = File::open("readme.md").unwrap();
    let mut stream = wl::os::UnixStream::connect("/run/user/1000/wayland-0")
        .unwrap();
    let mut message = wl::Message::new(1, 0);
    message.push_file(&file);
    message.send(&mut stream).unwrap();
}