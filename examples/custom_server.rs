use std::{fs::File, io::Read, os::unix::prelude::FromRawFd};
use wl::server::prelude::*;

fn main() {
    let server = wl::Server::bind().unwrap();
    server.start::<CInit, ErrorHandler>()
}

#[derive(Default)]
struct ErrorHandler;
impl DispatchErrorHandler for ErrorHandler {
    fn handle(&mut self, client: &mut Client, error: wl::DispatchError) -> Result<()> {
        let mut lease: Lease<CInit> = client.get(Client::DISPLAY)?;
        let message = format!("{}", error);
        {
            use custom::CInit;
            lease.error(client, &lease.object(), custom::CInitError::GENERIC, &message)
        }
    }
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