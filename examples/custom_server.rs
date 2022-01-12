use std::{fs::File, io::Read, os::unix::prelude::FromRawFd};
use wl::{server::prelude::*, Result};

fn main() {
    let server = wl::Server::bind().unwrap();
    server.start::<CInit>()
}

#[protocol("protocol/custom.toml")]
mod custom {
    type CInit = super::CInit;
}

#[derive(Default)]
pub struct CInit;
impl custom::CInit for Lease<CInit> {
    fn read(&mut self, _: &mut Client, fd: Fd) -> Result<()> {
        let mut string = String::new();
        let mut file = unsafe { File::from_raw_fd(fd.into()) };
        if let Err(e) = file.read_to_string(&mut string) {
            eprintln!("Failed to read file: {}", e);
        }
        println!("{}", string);
        Ok(())
    }
}