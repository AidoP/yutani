use std::{path::Path, any::Any, marker::PhantomData};

use crate::{prelude::*, wire::{self, *}};
use ahash::{HashMap, HashMapExt};
use syslib::Fd;

pub mod prelude {
    pub use crate::prelude::*;
    pub use super::{
        Server,
        Client
    };
}

pub struct Server<T, F: Fn(&mut EventLoop<T>, &mut Client<T>) -> Resident<dyn Any, T, Client<T>>> {
    server: wire::Server,
    constructor: F,
    _marker: PhantomData<T>
}
impl<T: 'static, F: 'static + Fn(&mut EventLoop<T>, &mut Client<T>) -> Resident<dyn Any, T, Client<T>>> Server<T, F> {
    #[inline]
    pub fn event_loop<P: AsRef<Path>>(state: T, path: P, constructor: F) -> Result<wire::EventLoop<T>> {
        wire::EventLoop::new(state).and_then(|mut event_loop| {
            let server = wire::Server::listen(path)
                .map(|server| Self { server, constructor, _marker: PhantomData })?;
            event_loop.add(Box::new(server))?;
            Ok(event_loop)
        })
    }
}
impl<T: 'static, F: Fn(&mut EventLoop<T>, &mut Client<T>) -> Resident<dyn Any, T, Client<T>>> EventSource<T> for Server<T, F> {
    fn fd(&self) -> Fd<'static> {
        self.server.socket.fd().extend()
    }

    fn input(&mut self, event_loop: &mut EventLoop<T>) -> Result<()> {
        let fd = syslib::accept(&self.server.socket);
        let stream = fd
            .map_err(Error::Sys)
            .and_then(Stream::new)
            .map(Client::new)
            .map(|mut client| {
                let display = (self.constructor)(event_loop, &mut client);
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
    objects: HashMap<Id, Resident<dyn Any, T, Self>>
}
impl<T> Client<T> {
    pub fn new(stream: Stream) -> Self {
        Self {
            stream,
            objects: HashMap::new()
        }
    }
    fn insert(&mut self, object: Resident<dyn Any, T, Self>) -> Result<()> {
        let id = object.id();
        if self.objects.insert(id, object).is_some() {
            Err(Error::DuplicateObject(id.into()))
        } else {
            Ok(())
        }
    }
    pub fn get(&mut self, id: Id) -> Result<Lease<dyn Any>> {
        self.objects.get_mut(&id).ok_or(Error::NoObject(id.into())).and_then(|r| r.lease())
    }
    pub fn remove(&mut self, id: Id) -> Option<Resident<dyn Any, T, Self>> {
        self.objects.remove(&id)

    }
    fn dispatch(&mut self) -> Result<()> {
        Ok(())
    }
}
impl<T> EventSource<T> for Client<T> {
    fn fd(&self) -> Fd<'static> {
        self.stream.socket.fd().extend()
    }

    fn input(&mut self, event_loop: &mut EventLoop<T>) -> Result<()> {
        if self.stream.recvmsg()? {
            self.dispatch()?
        }
        Ok(())
    }
}