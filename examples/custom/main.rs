use std::{os::unix::prelude::{RawFd, FromRawFd}, fs::File, io::Read};

use wl::{server::{self, Lease}, NewId};

fn main() {
    let server = wl::Server::<Protocol>::bind().unwrap();
    server.start()
}

#[server::protocol("custom.toml")]
pub enum Protocol {
    #[display]
    CInit(Init)
}
type Client = server::Client<Protocol>;

#[derive(Default)]
pub struct Init;
impl CInit for Lease<Init> {
    fn read(self, client: &mut Client, fd: RawFd) -> Option<Self> {
        unsafe {
            let mut file = File::from_raw_fd(fd);
            let mut string = String::new();
            if let Err(e) = file.read_to_string(&mut string) {
                eprintln!("Failed to read file: {}", e);
            }
            println!("{}", string);
        }
        Some(self)
    }
}