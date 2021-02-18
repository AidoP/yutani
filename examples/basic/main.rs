mod protocol;
use protocol::Protocol;

fn main() {
    let server = wl::Server::<Protocol>::bind().unwrap();
    server.start()
}