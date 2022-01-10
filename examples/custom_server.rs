use std::{fs::File, io::Read, os::unix::prelude::FromRawFd};

use wl::{Fd, server::{protocol, Lease, Client}};

fn main() {
    let server = wl::Server::bind().unwrap();
    server.start::<Init>()
}

#[derive(Default)]
pub struct Init;
#[protocol("custom.toml")]
impl CInit for Lease<Init> {
    fn read(&mut self, _: &mut Client, fd: Fd) -> wl::Result<()> {
        let mut string = String::new();
        let mut file = unsafe { File::from_raw_fd(fd.into()) };
        if let Err(e) = file.read_to_string(&mut string) {
            eprintln!("Failed to read file: {}", e);
        }
        println!("{}", string);
        Ok(())
    }
}