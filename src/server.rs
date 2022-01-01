use std::{
    collections::{HashMap, VecDeque},
    fs,
    fmt,
    io,
    marker::PhantomData,
    os::unix::{net::{UnixListener, UnixStream}, prelude::RawFd},
    ops::{Deref, DerefMut}
};

use crate::common::*;
pub use wl_macro::{server_protocol as protocol};

pub struct Server<P: Protocol>(UnixListener, PhantomData<P>);
impl<P: Protocol> Server<P> {
    pub fn bind() -> io::Result<Self> {
        let path = &get_socket_path();
        if let Ok(listener) = UnixListener::bind(path) {
            Ok(Self(listener, PhantomData))
        } else {
            // Ensure the UnixStream is dropped
            let is_err = { UnixStream::connect(path).is_err() };
            if is_err {
                fs::remove_file(path)?;
                UnixListener::bind(path).map(|listener| Self(listener, PhantomData))
            } else {
                Err(io::ErrorKind::AddrInUse.into())
            }
        }
    }
    pub fn start(self) -> ! {
        for stream in self.incoming() {
            let stream = stream.unwrap();
            let mut objects = HashMap::new();
            objects.insert(0, None);
            objects.insert(1, Some(P::default()));
            let mut client = Client {
                stream,
                messages: Default::default(),
                file_descriptors: Default::default(),
                objects
            };
            loop {
                if let Err(e) = client.dispatch() {
                    eprintln!("Dispatch Error: {:?}", e);
                    break
                }
            }
        }
        unreachable!()
    }
}
impl<P: Protocol> Deref for Server<P> {
    type Target = UnixListener;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl<P: Protocol> DerefMut for Server<P> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// A client that is connected to the server
///
/// Clients own their resources but may lease them out during dispatch
pub struct Client<P: Protocol> {
    stream: UnixStream,
    messages: RingBuffer,
    file_descriptors: VecDeque<RawFd>,
    objects: HashMap<u32, Option<P>>
}
impl<P: Protocol> Client<P> {
    /// Wait for the next message and execute it
    pub fn dispatch(&mut self) -> Result<()> {
        self.messages.receive(&mut self.file_descriptors, &self.stream)?;
        let message = Message::read(&mut self.messages)?;
        let lease = self.lease(message.object)?;
        let id = lease.id;
        if let Some(lease) = Protocol::request(lease, self, message)? {
            self.release(lease);
        } else {
            self.objects.remove(&id);
        }
        Ok(())
    }
    /// Send a message to the client
    pub fn send(&mut self, message: Message) -> Result<()> {
        Ok(message.send(&mut self.stream)?)
    }
    /// Send a global error event to the client
    pub fn error(&mut self, object: u32, error: u32, msg: &str) {
        let mut message = Message::new(1, 0);
        message.push_u32(object as _);
        message.push_u32(error as _);
        message.push_str(msg);
        message.send(&mut self.stream).unwrap();
    }
    /// Collect the file descriptors from the socket's ancillary data
    pub fn next_fd(&mut self) -> Result<RawFd> {
        self.file_descriptors.pop_front().ok_or(DispatchError::ExpectedArgument("fd"))
    }
    /// The id of the Display object
    pub const DISPLAY: u32 = 1;
    /// Borrow an object from the client
    pub fn borrow(&self, id: u32) -> Result<&P> {
        self.objects
            .get(&id)
            .ok_or(DispatchError::ObjectNotFound(id))?
            .as_ref()
            .ok_or(DispatchError::ObjectTaken(id))
    }
    /// Mutably borrow an object from the client
    pub fn borrow_mut(&mut self, id: u32) -> Result<&mut P> {
        self.objects
            .get_mut(&id)
            .ok_or(DispatchError::ObjectNotFound(id))?
            .as_mut()
            .ok_or(DispatchError::ObjectTaken(id))
    }
    /// Lease out an object from the client
    ///
    /// A leased object unusable by the dispatch system
    pub fn lease(&mut self, id: u32) -> Result<GenericLease<P>> {
        let object = self.objects
            .get_mut(&id)
            .ok_or(DispatchError::ObjectNotFound(id))?
            .take()
            .ok_or(DispatchError::ObjectTaken(id))?;
        Ok(GenericLease {
            object,
            id
        })
    }
    /// Release an object back to the dispatch system, ending the lease
    pub fn release<L: Into<GenericLease<P>>>(&mut self, lease: L) {
        let GenericLease { object, id } = lease.into();
        *self.objects.get_mut(&id).unwrap() = Some(object);
    }
    /// Forget an object exists, returning the owned resources, leased or otherwise
    pub fn forget<O: Object>(&mut self, object: O) -> Option<P> {
        self.objects.remove(&object.id()).flatten()
    }
    /// Reserves a place for an object, converting it to a lease
    pub fn reserve<T>(&mut self, NewId { id, ..}: NewId, object: T) -> Result<Lease<T>> {
        if let Some(_) = self.objects.insert(id, None) {
            Err(DispatchError::ObjectExists(id))
        } else {
            Ok(Lease {
                object,
                id
            })
        }
    }
    /// Insert an object
    ///
    /// Takes ownership of the object. To keep ownership in the form of a lease, use `reserve`
    pub fn insert(&mut self, NewId { id, ..}: NewId, object: P) -> Result<GenericLease<P>> {
        if let Some(_) = self.objects.insert(id, None) {
            Err(DispatchError::ObjectExists(id))
        } else {
            Ok(GenericLease {
                object,
                id
            })
        }
    }
    /// Converts a NewId to a lease without storing the lease.
    ///
    /// Useful if, and only if, the object is a temporary that is discarded without ever passing it to the client
    /// # Safety
    /// Attempting to release an upgraded lease will result in undefined behaviour
    // Note: Though semantically unsafe, it is safe Rust. unsafe abuse?
    pub unsafe fn upgrade<T>(&mut self, NewId { id, ..}: NewId, object: T) -> Lease<T> {
        Lease {
            object,
            id
        }
    }
}
/// A contract relinquishment of an object by the client with the promise of its return
pub struct GenericLease<P: Protocol> {
    object: P,
    pub id: u32
}
impl<P: Protocol> GenericLease<P> {
    pub fn try_map<F: Fn(P) -> Result<T>, T>(self, f: F) -> Result<Lease<T>> {
        let Self { object, id } = self;
        f(object).map(|object| Lease {
            object,
            id
        })
    }
    pub fn map<F: Fn(P) -> T, T>(self, f: F) -> Lease<T> {
        Lease {
            object: f(self.object),
            id: self.id
        }
    }
}
impl<P: Protocol> Deref for GenericLease<P> {
    type Target = P;
    fn deref(&self) -> &Self::Target {
        &self.object
    }
}
impl<P: Protocol> DerefMut for GenericLease<P> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.object
    }
}
impl<P: Protocol, T: Into<P>> From<Lease<T>> for GenericLease<P> {
    fn from(Lease { object, id }: Lease<T>) -> Self {
        Self {
            object: object.into(),
            id
        }
    }
}
impl<P: Protocol> fmt::Debug for GenericLease<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.id)
    }
}
/// A lease with the concrete type extracted.
///
/// Leases are created in glue code where the concrete types of a generic lease are known.
pub struct Lease<T> {
    object: T,
    pub id: u32
}
impl<T> Lease<T> {
    pub fn map<F: Fn(T) -> P, P: Protocol>(self, f: F) -> GenericLease<P> {
        GenericLease {
            object: f(self.object),
            id: self.id
        }
    }
}
impl<T> Deref for Lease<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.object
    }
}
impl<T> DerefMut for Lease<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.object
    }
}

/// Allows the creation of messages by a leased object without leaking the object id
pub trait Object {
    fn id(&self) -> u32;
}
impl<T> Object for Lease<T> {
    fn id(&self) -> u32 {
        self.id
    }
}
impl<P: Protocol> Object for GenericLease<P> {
    fn id(&self) -> u32 {
        self.id
    }
}

/// A protocol defines a set of interfaces in use by the Wayland IPC system.
/// In terms of this crate, the protocol trait allows the dispatch system to store a generic, and unknown to the `wl` crate,
/// set of interfaces that the glue macros can use to store static type information for otherwise generic objects.
///
/// Use the `#[wl::server::protocol]` attribute macro to create the enum representing concrete interface types
pub trait Protocol: Default + Sized {
    /// Call a function on this object for a given message
    fn request(lease: GenericLease<Self>, client: &mut Client<Self>, message: Message) -> Result<Option<GenericLease<Self>>>;
    /// The interface name for this object instance
    fn interface(&self) -> &'static str;
}