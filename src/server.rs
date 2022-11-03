use std::{path::Path, any::Any, marker::PhantomData};

use crate::{prelude::*, wire::{self, *}};
use ahash::{HashMap, HashMapExt};
use syslib::Fd;

pub mod prelude {
    pub use crate::prelude::*;
    pub use super::{
        Server,
        Client,
        Resident
    };
}

pub type Resident<T> = crate::lease::Resident<dyn Any, T, Client<T>>;
pub type GlobalBuilderFn<T> = fn(&mut EventLoop<T>, &mut Client<T>, u32) -> Resident<T>;

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
    #[inline]
    pub fn event_loop<P: AsRef<Path>>(state: T, path: P, constructor: GlobalBuilderFn<T>) -> crate::Result<wire::EventLoop<T>> {
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
                let display = (self.constructor)(event_loop, &mut client, 1);
                client.insert(display).unwrap();
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
    registry: Vec<Global<T>>,
    objects: HashMap<Id, Resident<T>>,
    new_id: u32
}
impl<T> Client<T> {
    pub fn new(stream: Stream) -> Self {
        Self {
            stream,
            registry: Vec::new(),
            objects: HashMap::new(),
            new_id: 0xFF00_0000
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
    /// Register a global object constructor.
    pub fn register(&mut self, global: Global<T>) {
        self.registry.push(global)
    }
    pub fn globals(&self) -> &[Global<T>] {
        &self.registry
    }
    pub fn create_global(&mut self, event_loop: &mut EventLoop<T>, global: u32, id: NewId) -> Result<Lease<dyn Any>, WlError<'static>> {
        let global = self.registry.get(global as usize).ok_or(WlError::NO_GLOBAL)?;
        let mut object = (global.constructor)(event_loop, self, id.version());
        let lease = object.lease().ok_or(WlError::INTERNAL)?;
        self.insert(object)?;
        Ok(lease)
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
        let key = self.stream.send_message(Id::DISPLAY, 1)?;
        self.stream.send_object(id)?;
        self.stream.commit(key)?;
        Ok(resident)
    }
    /// Send a protocol error to the client.
    pub fn error(&mut self, error: &WlError) -> Result<(), WlError> {
        let key = self.stream.send_message(Id::DISPLAY, 0)?;
        self.stream.send_object(error.object)?;
        self.stream.send_u32(error.error)?;
        self.stream.send_string(&error.description)?;
        self.stream.commit(key)?;
        Ok(())
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