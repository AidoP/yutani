use std::{fs::File, io::Read};
use wl::server::prelude::*;

fn main() {
    let server = wl::Server::bind().unwrap();
    server.start(CInit::default(), ErrorHandler::default(), |_, _| Ok(()))
}

#[derive(Default, Clone)]
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

#[derive(Default, Clone)]
pub struct CInit;
impl custom::CInit for Lease<CInit> {
    fn read(&mut self, _: &mut Client, mut file: File) -> Result<()> {
        let mut string = String::new();
        if let Err(e) = file.read_to_string(&mut string) {
            eprintln!("Failed to read file: {}", e);
        }
        println!("{}", string);
        Ok(())
    }
}