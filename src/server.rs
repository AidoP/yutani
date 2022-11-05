use std::{path::Path, any::Any, marker::PhantomData};

use crate::{prelude::*, wire::{self, *}};
use ahash::{HashMap, HashMapExt};
use syslib::Fd;

pub mod prelude {
    //pub use wl_macro::server_protocol as protocol;
    pub use crate::prelude::*;
    pub use super::{
        Server,
        Client,
        Resident
    };
}

pub type Resident<T> = crate::lease::Resident<dyn Any, T, Client<T>>;
pub type GlobalBuilderFn<T> = fn(&mut EventLoop<T>, &mut Client<T>, Id, u32) -> Result<Resident<T>, WlError<'static>>;

pub struct Global<T> {
    pub interface: &'static str,
    pub version: u32,
    pub constructor: GlobalBuilderFn<T>
}

pub struct Server<T> {
    server: wire::Server,
    constructor: GlobalBuilderFn<T>,
    _marker: PhantomData<T>
}
impl<T: 'static> Server<T> {
    /// Create an event loop with a `wl::Server` server attached as an event source.
    /// The server will bind and listen to the Unix Domain socket at the specified path.
    /// The `EventLoop` will contain the specified global state.
    /// 
    /// When a client connects to the socket a new `wl::server::Client` instance will be created and
    /// attached as an event source on the `EventLoop`.
    #[inline]
    pub fn event_loop<P: AsRef<Path>>(path: P, state: T, constructor: GlobalBuilderFn<T>) -> crate::Result<wire::EventLoop<T>> {
        wire::EventLoop::new(state).and_then(|mut event_loop| {
            let server = wire::Server::listen(path)
                .map(|server| Self { server, constructor, _marker: PhantomData })?;
            event_loop.add(Box::new(server))?;
            Ok(event_loop)
        })
    }
}
impl<T: 'static> EventSource<T> for Server<T> {
    fn fd(&self) -> Fd<'static> {
        self.server.socket.fd().extend()
    }

    fn input(&mut self, event_loop: &mut EventLoop<T>) -> crate::Result<()> {
        let fd = syslib::accept(&self.server.socket);
        let stream = fd
            .map_err(Error::Sys)
            .and_then(Stream::new)
            .map(Client::new)
            .map(|mut client| {
                let display = (self.constructor)(event_loop, &mut client, Id::new(1), 1);
                client.insert(display.unwrap()).unwrap();
                Box::new(client)
            });
        match stream {
            Ok(stream) => if let Err(e) = event_loop.add(stream) {
                eprintln!("Failed to add new client to the event loop: {:?}", e)
            },
            Err(e) => eprintln!("Failed to accept new client: {:?}", e)
        }
        Ok(())
    }
}

pub struct Client<T> {
    stream: Stream,
    objects: HashMap<Id, Resident<T>>,
    new_id: u32,
    event_serial: u32
}
impl<T> Client<T> {
    pub fn new(stream: Stream) -> Self {
        Self {
            stream,
            objects: HashMap::new(),
            new_id: 0xFF00_0000,
            event_serial: 0
        }
    }
    pub fn stream(&mut self) -> &mut Stream {
        &mut self.stream
    }
    /// Get a new ID suitable for the next object.
    /// Failure to create an object with the id may be considered a protocol error under `libwayland`.
    pub fn new_id(&mut self) -> u32 {
        let id = self.new_id;
        self.new_id = self.new_id.checked_add(1).unwrap_or(0xFF00_0000);
        id
    }
    /// Get the event serial, then increment it.
    pub fn next_event(&mut self) -> u32 {
        let event_serial = self.event_serial;
        self.event_serial = self.event_serial.wrapping_add(1);
        event_serial
    }
    /// Insert an object in to the client.
    pub fn insert(&mut self, object: Resident<T>) -> Result<(), WlError<'static>> {
        let id = object.id();
        if self.objects.insert(id, object).is_some() {
            Err(WlError::INTERNAL)
        } else {
            Ok(())
        }
    }
    pub fn remove(&mut self, id: Id) -> Result<Resident<T>, WlError<'static>> {
        let resident = self.objects.remove(&id).ok_or(WlError::NO_OBJECT)?;
        let key = self.stream.start_message(Id::DISPLAY, 1);
        self.stream.send_object(Some(id))?;
        self.stream.commit(key)?;
        Ok(resident)
    }
    /// Send a protocol error to the client.
    pub fn error(&mut self, error: &WlError) -> Result<(), WlError> {
        let key = self.stream.start_message(Id::DISPLAY, 0);
        self.stream.send_object(Some(error.object))?;
        self.stream.send_u32(error.error)?;
        self.stream.send_string(Some(&error.description))?;
        self.stream.commit(key)
    }
    pub fn get_mut(&mut self, id: Id) -> Option<&mut Resident<T>> {
        self.objects.get_mut(&id)
    }
    pub fn lease(&mut self, id: Id) -> Result<Lease<dyn Any>, WlError<'static>> {
        self.objects.get_mut(&id).and_then(Resident::lease).ok_or(WlError::INTERNAL)
    }
}
impl<T> EventSource<T> for Client<T> {
    fn fd(&self) -> Fd<'static> {
        self.stream.socket.fd().extend()
    }

    fn input(&mut self, event_loop: &mut EventLoop<T>) -> crate::Result<()> {
        let result = if self.stream.recvmsg()? {
            let dispatch_result = (|| {
                while let Some(message) = self.stream.message() {
                    let message = message?;
                    if let Some(resident) = self.get_mut(message.object) {
                        let dispatch = resident.dispatch();
                        let lease = resident.lease().ok_or(WlError::INTERNAL)?;
                        dispatch(lease, event_loop, self, message)?
                    } else {
                        // TODO: if the object was recently deleted just ignore the request as requests may have been in-flight still
                        return Err(WlError::NO_OBJECT)
                    }
                }
                Ok(())
            })();
            if let Err(error) = dispatch_result {
                let _ = self.error(&error);
                Err(Error::Protocol(error))
            } else {
                Ok(())
            }
        } else {
            Ok(())
        };
        self.stream.sendmsg()?;
        result
    }
}