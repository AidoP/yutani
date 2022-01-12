use std::{
    collections::{HashMap, VecDeque},
    fs,
    io,
    ops::{Deref, DerefMut}, any::Any, fmt::{self, Display}
};

use crate::common::*;
pub use wl_macro::{server_protocol as protocol};

pub mod prelude {
    pub use crate::{
        types::*,
        Object,
        server::{
            Result,
            ErrorHandler,
            DispatchErrorHandler,
            Lease,
            Server,
            Client,
            Dispatch,
            protocol
        }
    };
}

/// A server implementing the Wayland wire protocol and a higher-level protocol with the entry point given in `Server::start::<Interface>()`
pub struct Server(UnixListener);
impl Server {
    pub fn bind() -> io::Result<Self> {
        let path = &get_socket_path();
        if let Ok(listener) = UnixListener::bind(path) {
            Ok(Self(listener))
        } else {
            // Ensure the UnixStream is dropped
            let is_err = { UnixStream::connect(path).is_err() };
            if is_err {
                fs::remove_file(path)?;
                UnixListener::bind(path).map(|listener| Self(listener))
            } else {
                Err(io::ErrorKind::AddrInUse.into())
            }
        }
    }
    pub fn start<T: 'static + Dispatch + Default, E: 'static + DispatchErrorHandler + Default>(self) -> ! {
        for stream in self.0 {
            //std::thread::spawn(|| {
                let mut client = Client {
                    stream,
                    messages: Default::default(),
                    fds: Default::default(),
                    objects: Default::default(),
                    error_handler: Some(Box::new(E::default())),
                    serial: 0
                };
                client.add(Null).unwrap();
                client.add(T::default()).unwrap();
                loop {
                    if let Err(e) = client.dispatch() {
                        if let Err(e) = e.try_handle(&mut client) {
                            eprintln!("{}", e);
                            break
                        }
                    }
                }
            //});
        }
        unreachable!()
    }
}

/// The representation of the client connected to the server
///
/// Messages are processed on objects which implement an interface
pub struct Client {
    stream: UnixStream,
    messages: RingBuffer,
    // TODO: Consider limiting. As is, a client can send FD's until the server is starved, causing a DoS
    fds: VecDeque<Fd>,
    objects: HashMap<u32, Resident<dyn Any>>,
    error_handler: Option<Box<dyn DispatchErrorHandler>>,
    /// A counter for generating unique ID's
    serial: u32
}
impl Client {
    /// Collect any new messages and execute them
    pub fn dispatch(&mut self) -> Result<()> {
        if self.stream.recvmsg(&mut self.messages, &mut self.fds)? {
            while Message::available(&self.messages) {
                let message = Message::read(&mut self.messages)?;
                self.get_any(message.object)?.dispatch(self, message)?;
            }
        }
        Ok(())
    }
    /// Send a message down the wire 
    pub fn send(&mut self, message: Message) -> Result<()> {
        Ok(message.send(&mut self.stream)?)
    }
    /* TODO: Allow the user to specify an error reporting function
    /// Send a global error event to the client
    pub fn error(&mut self, object: u32, error: u32, msg: &str) {
        let mut message = Message::new(1, 0);
        message.push_u32(object as _);
        message.push_u32(error as _);
        message.push_str(msg);
        message.send(&mut self.stream).unwrap();
    }*/
    /// Get the next available file descriptor from the queue
    pub fn next_fd(&mut self) -> std::result::Result<Fd, DispatchError> {
        self.fds.pop_front().ok_or(DispatchError::ExpectedArgument { data_type: "fd" })
    }
    /// The id of the Display object
    pub const DISPLAY: u32 = 1;
    /// Borrow an object from the client
    pub fn get<T: 'static + Dispatch>(&self, id: u32) -> Result<Lease<T>> {
        if let Some(object) = self.objects.get(&id) {
            object.lease(id)?.downcast()
        } else {
            Err(DispatchError::ObjectNotFound(id).into())
        }
    }
    /// Borrow an object from the client, not knowing the static type
    pub fn get_any(&self, id: u32) -> Result<Lease<dyn Any>> {
        if let Some(object) = self.objects.get(&id) {
            object.lease(id)
        } else {
            Err(DispatchError::ObjectNotFound(id).into())
        }
    }
    /// Attempt to insert an object for the given ID
    pub fn insert<T: 'static + Dispatch>(&mut self, id: impl Object, object: T) -> Result<Lease<T>> {
        let id = id.object();
        if self.objects.contains_key(&id) {
            Err(DispatchError::ObjectExists(id).into())
        } else {
            let object = Resident::new(object);
            let lease = object.lease(id)?;
            self.objects.insert(id, object);
            //Dispatch::init(&mut lease, self)?;
            Ok(lease)
        }
    }
    /// Insert an object with the next available ID 
    #[inline]
    pub fn add<T: 'static + Dispatch>(&mut self, object: T) -> Result<Lease<T>> {
        let id = self.new_id();
        self.insert(id, object)
    }
    /// Attempts to generate an available object ID
    /// Only guarantees that it is unique for the resident objects at the time of calling,
    /// in-flight objects must be registered before the serial wraps around
    pub fn new_id(&mut self) -> u32 {
        // There must be a better way
        while self.objects.contains_key(&self.serial) {
            self.serial = self.serial.wrapping_add(1);
        }
        self.serial
    }
    /// Remove an object from the client by lease
    pub fn drop<T: ?Sized>(&mut self, lease: &mut Lease<T>) -> Result<()> {
        if let Some(_) = self.objects.remove(&lease.id) {
            Ok(())
        } else {
            Err(DispatchError::ObjectNotFound(lease.id).into())
        }
    }
    /// Remove an object from the client, returning a lease
    pub fn remove<T: 'static + Dispatch>(&mut self, id: u32) -> Result<Lease<T>> {
        if let Some(r) = self.objects.remove(&id) {
            r.lease(id)
                .and_then(|l| l.downcast())
        } else {
            Err(DispatchError::ObjectNotFound(id).into())
        }
    }
    /// Create an object that is never stored
    pub fn temporary<T: Dispatch>(&mut self, NewId { id, ..}: NewId, object: T) -> Result<Lease<T>> {
        if self.objects.contains_key(&id) {
            Err(DispatchError::ObjectExists(id).into())
        } else {
            let lease = Lease::temporary(id, object);
            //Dispatch::init(&mut lease, self)?;
            Ok(lease)
        }
    }
    /// Attempt to handle a dispatch error using the registered error handler
    fn handle(&mut self, error: DispatchError) -> Result<()> {
        if let Some(mut handler) = self.error_handler.take() {
            handler.handle(self, error)?;
            self.error_handler = Some(handler);
            Ok(())
        } else {
            Err(SystemError::NoDispatchHandler(error).into())
        }
    }
}

#[repr(C)]
pub struct LeaseBox<T: ?Sized> {
    leased: bool,
    interface: &'static str,
    version: u32,
    dispatch: fn(Lease<dyn Any>, &mut Client, Message) -> Result<()>,
    value: T
}

// TODO: Can trait object coersion be implemented in stable?
impl<T: ?Sized + std::marker::Unsize<U>, U: ?Sized> std::ops::CoerceUnsized<Resident<U>> for Resident<T> {}
impl<T: ?Sized + std::marker::Unsize<U>, U: ?Sized> std::ops::DispatchFromDyn<Resident<U>> for Resident<T> {}
pub struct Resident<T: ?Sized> {
    ptr: *mut LeaseBox<T>
}
impl<T: Dispatch> Resident<T> {
    fn new(value: T) -> Self {
        Self {
            ptr: Box::leak(box LeaseBox { leased: false, interface: T::INTERFACE, version: T::VERSION, dispatch: T::dispatch, value })
        }
    }
}
impl<T: ?Sized> Resident<T> {
    pub fn as_ref(&self) -> Option<&T> {
        unsafe {
            if (*self.ptr).leased {
                None
            } else {
                Some(&(*self.ptr).value)
            }
        }
    }
    pub fn as_mut(&mut self) -> Option<&mut T> {
        unsafe {
            if (*self.ptr).leased {
                None
            } else {
                Some(&mut (*self.ptr).value)
            }
        }
    }
    fn lease(&self, id: u32) -> Result<Lease<T>> {
        unsafe {
            if (*self.ptr).leased {
                Err(SystemError::ObjectLeased(id).into())
            } else {
                (*self.ptr).leased = true;
                Ok(Lease { ptr: self.ptr, id })
            }
        }
    }
}
impl<T: ?Sized> Drop for Resident<T> {
    fn drop(&mut self) {
        unsafe {
            if (*self.ptr).leased {
                (*self.ptr).leased = false;
            } else {
                Box::from_raw(self.ptr);
            }
        }
    }
}

/// Allows the creation of messages by a leased object without leaking the object id
pub struct Lease<T: ?Sized> {
    ptr: *mut LeaseBox<T>,
    id: u32
}
impl<T: Dispatch> Lease<T> {
    /// Creates a lease that will never have a corresponding resident
    fn temporary(id: u32, value: T) -> Self {
        Self {
            ptr: Box::leak(box LeaseBox { leased: false, interface: T::INTERFACE, version: T::VERSION, dispatch: T::dispatch, value }),
            id
        }
    }
}
impl Lease<dyn Any> {
    /// Downcast the dynamic object to a concrete type
    pub fn downcast<T: Dispatch + Any>(self) -> Result<Lease<T>> {
        if self.is::<T>() {
            let ptr = self.ptr.cast();
            let id = self.id;
            std::mem::forget(self);
            Ok(Lease { id, ptr })
        } else {
            Err(DispatchError::UnexpectedObjectType {
                object: self.id,
                had_interface: self.interface(),
                expected_interface: T::INTERFACE
            }.into())
        }
    }
    #[inline]
    fn dispatch(self, client: &mut Client, message: Message) -> Result<()> {
        unsafe {
            ((*self.ptr).dispatch)(self, client, message)
        }
    }
    pub fn interface(&self) -> &'static str {
        unsafe { (*self.ptr).interface }
    }
    pub fn version(&self) -> u32 {
        unsafe { (*self.ptr).version }
    }
}
impl<T: ?Sized> Deref for Lease<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &(*self.ptr).value }
    }
}
impl<T: ?Sized> DerefMut for Lease<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut (*self.ptr).value }
    }
}
impl<T: ?Sized> Display for Lease<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "object {}", self.id)
    }
}
impl<T: ?Sized> Drop for Lease<T> {
    fn drop(&mut self) {
        unsafe {
            if (*self.ptr).leased {
                (*self.ptr).leased = false;
            } else {
                Box::from_raw(self.ptr);
            }
        }
    }
}

/// Dispatch allows an interface to describe how to decode a message and execute concrete request implementations.
/// 
/// Use the `#[wl::server::protocol]` attribute macro to create dispatch glue code.
pub trait Dispatch {
    const INTERFACE: &'static str;
    const VERSION: u32;
    fn dispatch(lease: Lease<dyn Any>, client: &mut Client, message: Message) -> Result<()>;
    //fn init(lease: &mut Lease<Self>, client: &mut Client) -> Result<()>;
}
impl<T: ?Sized> Object for Lease<T> {
    fn object(&self) -> u32 {
        self.id
    }
}

pub struct Null;
impl Dispatch for Null {
    const INTERFACE: &'static str = "null";
    const VERSION: u32 = 0;
    fn dispatch(_: Lease<dyn Any>, _: &mut Client, _: Message) -> Result<()> {
        Err(DispatchError::ObjectNull.into())
    }
    //fn init(_: &mut Lease<Self>, _: &mut Client) -> Result<()>{ Ok(()) }
}


pub type Result<T> = std::result::Result<T, Error>;
pub trait ErrorHandler: fmt::Display {
    fn handle(&mut self, client: &mut Client) -> Result<()>;
}
pub trait DispatchErrorHandler {
    fn handle(&mut self, client: &mut Client, error: DispatchError) -> Result<()>;
}
pub enum Error {
    /// An error that originates outside of the library, in protocol code
    Protocol(Box<dyn ErrorHandler>),
    /// An error that occurs during dispatch and can be handled by a user-designated error handler
    Dispatch(DispatchError),
    /// An error indicating that the connection to the client must be severed
    System(SystemError)
}
impl Error {
    fn try_handle(self, client: &mut Client) -> Result<()> {
        match self {
            Self::Protocol(mut handler) => handler.handle(client),
            Self::Dispatch(error) => client.handle(error),
            Self::System(error) => Err(Self::System(error)),
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protocol(error) => write!(f, "Protocol error could not be handled, {}", error),
            Self::Dispatch(error) => write!(f, "Error during internal message handling, {}", error),
            Self::System(error) => write!(f, "Unrecoverable error, {}", error)
        }
    }
}
impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl From<SystemError> for Error {
    fn from(error: SystemError) -> Self {
        Error::System(error)
    }
}
impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::System(error.into())
    }
}
impl From<DispatchError> for Error {
    fn from(error: DispatchError) -> Self {
        Error::Dispatch(error)
    }
}