use std::{
    collections::{HashMap, VecDeque},
    fs::File,
    io,
    ops::{Deref, DerefMut}, any::Any, fmt::{self, Display}
};

use crate::common::*;
pub use wl_macro::{server_protocol as protocol};

pub mod prelude {
    pub use crate::{
        types::*,
        Event,
        EventListener,
        Events,
        Object,
        Nullable,
        server::{
            Error,
            Result,
            ErrorHandler,
            DispatchErrorHandler,
            Resident,
            Lease,
            Server,
            Client,
            Dispatch,
            protocol
        }
    };
}

/// A server implementing the Wayland wire protocol and a higher-level protocol with the entry point given in `Server::start::<Interface>()`
pub struct Server;
impl Server {
    pub fn listen<Display, Error, DropHandler>(display: Display, error_handler: Error, drop_handler: DropHandler) -> io::Result<Box<dyn Event>>
    where
        Display: 'static + Clone + Dispatch,
        Error: 'static + Clone + DispatchErrorHandler,
        DropHandler: 'static + Clone + Fn(&mut Client, Lease<dyn Any>) -> Result<()>
    {
        let listener = UnixListener::bind(get_socket_path(false)?)?;
        Ok(listener.on_accept(move |stream| {
            let client = Client::new(stream, display.clone(), error_handler.clone(), drop_handler.clone());
            Box::new(client)
        }))
    }
}

/// The representation of the client connected to the server
///
/// Messages are processed on objects which implement an interface
pub struct Client {
    stream: UnixStream,
    messages: RingBuffer,
    // TODO: Consider limiting. As is, a client can send FD's until the server is starved, causing a DoS
    fds: VecDeque<File>,
    objects: HashMap<u32, Resident<dyn Any>>,
    error_handler: Option<Box<dyn DispatchErrorHandler>>,
    /// A counter for generating unique ID's
    serial: u32,
    /// Objects that are queued for deletion
    drop_queue: Vec<u32>,
    drop_handler: Option<Box<dyn Fn(&mut Client, Lease<dyn Any>) -> Result<()>>>
}
impl Client {
    pub fn new<Display, Error, DropHandler>(stream: UnixStream, display: Display, error_handler: Error, drop_handler: DropHandler) -> Self
    where
        Display: 'static + Clone + Dispatch,
        Error: 'static + Clone + DispatchErrorHandler,
        DropHandler: 'static + Clone + Fn(&mut Client, Lease<dyn Any>) -> Result<()>
    {
        let mut client = Self {
            stream,
            messages: Default::default(),
            fds: Default::default(),
            objects: Default::default(),
            error_handler: Some(Box::new(error_handler)),
            serial: 0,
            drop_handler: Some(Box::new(drop_handler)),
            drop_queue: Default::default()
        };
        client.add(Null).unwrap();
        client.add(display).unwrap();
        client
    }
    /// Send a message down the wire 
    pub fn send(&mut self, message: Message) -> Result<()> {
        Ok(message.send(&mut self.stream)?)
    }
    /// Get the next available file descriptor from the queue
    pub fn next_file(&mut self) -> std::result::Result<File, DispatchError> {
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
    pub fn insert<T: 'static + Dispatch>(&mut self, new_id: NewId, object: T) -> Result<Lease<T>> {
        let id = new_id.object();
        if self.objects.contains_key(&id) {
            Err(DispatchError::ObjectExists(id).into())
        } else {
            let object = Resident::new(object, new_id.version);
            let lease = object.lease(id)?;
            self.objects.insert(id, object.into_any());
            //Dispatch::init(&mut lease, self)?;
            Ok(lease)
        }
    }
    /// Create a resident object that has discarded concrete type information
    #[inline]
    pub fn reserve<T: 'static + Dispatch>(object: T) -> Resident<dyn Any> {
        Resident::new(object, T::VERSION).into_any()
    }
    /// Attempt to insert an object that has been reserved as a resident
    pub fn insert_any(&mut self, id: NewId, object: Resident<dyn Any>) -> Result<Lease<dyn Any>> {
        let id = id.object();
        if self.objects.contains_key(&id) {
            Err(DispatchError::ObjectExists(id).into())
        } else {
            let lease = object.lease(id)?;
            self.objects.insert(id, object);
            Ok(lease)
        }
    }
    /// Insert an object with the next available ID 
    #[inline]
    pub fn add<T: 'static + Dispatch>(&mut self, object: T) -> Result<Lease<T>> {
        let id = self.new_id();
        self.insert(NewId::unknown(id), object)
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
    /// Queue an object for deletion. The object may not be deleted immediately.
    pub fn delete(&mut self, object: &dyn Object) -> Result<()> {
        if self.objects.contains_key(&object.object()) {
            self.drop_queue.push(object.object());
            Ok(())
        } else {
            Err(DispatchError::ObjectNotFound(object.object()).into())
        }
    }
    /// Drop all objects in the drop queue
    fn drop(&mut self) -> Result<()> {
        let drop_handler = self.drop_handler.take().unwrap();
        while let Some(id) = self.drop_queue.pop() {
            if let Some(object) = self.objects.remove(&id) {
                match object.lease(id).and_then(|object| drop_handler(self, object)) {
                    Ok(()) => (),
                    Err(e) => {
                        self.drop_handler = Some(drop_handler);
                        return Err(e)
                    }
                }
            }
        }
        self.drop_handler = Some(drop_handler);
        Ok(())
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
impl crate::os::Event for Client {
    fn fd(&self) -> &Fd {
        self.stream.fd()
    }
    fn events(&self) -> Events {
        Events::INPUT
    }
    fn signal(&mut self, events: Events, event_listener: &mut EventListener) {
        if events.hangup() {
            return event_listener.remove(self)
        }
        if events.input() {
            let mut dispatch = || -> Result<()> {
                self.stream.recvmsg(&mut self.messages, &mut self.fds)?;
                while Message::available(&self.messages) {
                    let message = Message::read(&mut self.messages)?;
                    self.get_any(message.object)?.dispatch(self, message)?;
                }
                // Drop objects queued for deletion
                self.drop()
            };
            if let Err(e) = dispatch() {
                if let Err(e) = e.try_handle(self) {
                    event_listener.remove(self);
                    eprintln!("{}", e);
                }
            }
        }
    }
}

#[repr(C)]
struct LeaseBox<T: ?Sized> {
    // TODO: though it can only be used on one thread, out-of-order execution could cause the lease flag to be out-of-date
    leased: bool,
    interface: &'static str,
    version: u32,
    dispatch: fn(Lease<dyn Any>, &mut Client, Message) -> Result<()>,
    value: T
}
/// `Resident` and `Lease` are asymmetric shared pointers
/// 
/// While `Lease` exists temporarily and allows access to the inner type, T, `Resident` exists only to create new `Lease`s and prevent the backing storage from dropping.
/// Single-bit reference counting ensures only 1 `Lease` is ever created at a time. This allows a mutable borrow to instead claim ownership briefly so that the `Client`
/// can be mutably borrowed by a Lease that it indirectly owns.
/// 
/// Inspired by `Option::take` which allows the same system without the automatic return of ownership.
pub struct Resident<T: ?Sized> {
    ptr: *mut LeaseBox<T>
}
impl<T: Dispatch> Resident<T> {
    fn new(value: T, version: u32) -> Self {
        Self {
            ptr: Box::leak(Box::new(LeaseBox { leased: false, interface: T::INTERFACE, version, dispatch: T::dispatch, value }))
        }
    }
}
impl<T: 'static + Any> Resident<T> {
    pub fn into_any(self) -> Resident<dyn Any> {
        let this: Resident<dyn Any> = Resident {
            ptr: self.ptr
        };
        std::mem::forget(self);
        this
    }
}
impl<T: ?Sized> Resident<T> {
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
impl<T: Clone + ?Sized> Clone for Resident<T> {
    fn clone(&self) -> Self {
        let other = unsafe { &*self.ptr };
        Self {
            ptr: Box::leak(Box::new(LeaseBox {
                leased: false,
                interface: other.interface,
                version: other.version,
                dispatch: other.dispatch,
                value: other.value.clone()
            }))
        }
    }
}
impl<T: ?Sized> Drop for Resident<T> {
    fn drop(&mut self) {
        unsafe {
            if (*self.ptr).leased {
                (*self.ptr).leased = false;
            } else {
                drop(Box::from_raw(self.ptr));
            }
        }
    }
}

/// A temporary claim of ownership of an object. Ownership will be returned implicitly to the corresponding `Resident` on drop.
pub struct Lease<T: ?Sized> {
    ptr: *mut LeaseBox<T>,
    id: u32
}
impl<T: 'static + Any> Lease<T> {
    pub fn into_any(self) -> Lease<dyn Any> {
        let this: Lease<dyn Any> = Lease {
            ptr: self.ptr,
            id: self.id
        };
        std::mem::forget(self);
        this
    }
}
impl<T: 'static + ?Sized + Any> Lease<T> {
    pub fn interface(&self) -> &'static str {
        unsafe { (*self.ptr).interface }
    }
    pub fn version(&self) -> u32 {
        unsafe { (*self.ptr).version }
    }
}
impl Lease<dyn Any> {
    /// Downcast the dynamic object to a concrete type
    pub fn downcast<T: Dispatch + Any>(self) -> Result<Lease<T>> {
        if unsafe { (*self.ptr).value.is::<T>() } {
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
}
impl<T> Deref for Lease<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &(*self.ptr).value }
    }
}
impl<T> DerefMut for Lease<T> {
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
                drop(Box::from_raw(self.ptr));
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