use std::{
    collections::{HashMap, VecDeque},
    fs::{self, File},
    io,
    ops::{Deref, DerefMut}, any::Any, fmt::{self, Display}, path::Iter, time::{Duration, Instant}
};

use crate::common::*;
pub use wl_macro::{server_protocol as protocol};

pub mod prelude {
    pub use crate::{
        types::*,
        Object,
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
pub struct Server(UnixListener);
impl Server {
    pub fn bind() -> io::Result<Self> {
        UnixListener::bind(get_socket_path(false)?)
            .map(|listener| Self(listener))
    }
    pub fn start<Display: 'static + Dispatch + Send + Clone, Error: 'static + DispatchErrorHandler + Send + Clone>(self, display: Display, error_handler: Error) {
        for stream in self.0 {
            let display = display.clone();
            let error_handler = error_handler.clone();
            std::thread::spawn(|| {
                let mut client = Client {
                    stream,
                    messages: Default::default(),
                    fds: Default::default(),
                    objects: Default::default(),
                    error_handler: Some(Box::new(error_handler)),
                    timer_events: Some(Default::default()),
                    generic_events: Some(Default::default()),
                    serial: 0
                };
                client.add(Null).unwrap();
                client.add(display).unwrap();
                loop {
                    if let Err(e) = client.dispatch() {
                        if let Err(e) = e.try_handle(&mut client) {
                            eprintln!("{}", e);
                            break
                        }
                    }
                }
            });
        }
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
    timer_events: Option<TimerList>,
    generic_events: Option<EventList>,
    /// A counter for generating unique ID's
    serial: u32
}
impl Client {
    /// Collect any new messages and execute them, then signal all ready events
    pub fn dispatch(&mut self) -> Result<()> {
        if self.stream.poll() && self.stream.recvmsg(&mut self.messages, &mut self.fds)? {
            while Message::available(&self.messages) {
                let message = Message::read(&mut self.messages)?;
                self.get_any(message.object)?.dispatch(self, message)?;
            }
        }
        // Signal all timer events. Remove the list from the client to prevent a mutable reference cycle
        let mut timer_events = self.timer_events.take();
        if let Some(timer_events) = &mut timer_events {
            timer_events.signal_ready(self)?
        }
        self.timer_events = timer_events;
        // Signal all other events. Remove the list from the client to prevent a mutable reference cycle
        self.generic_events = self.generic_events.take();
        let mut generic_events = self.generic_events.take();
        if let Some(generic_events) = &mut generic_events {
            generic_events.signal_ready(self)?
        }
        self.generic_events = generic_events;
        Ok(())
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
    pub fn insert<T: 'static + Dispatch>(&mut self, id: impl Object, object: T) -> Result<Lease<T>> {
        let id = id.object();
        if self.objects.contains_key(&id) {
            Err(DispatchError::ObjectExists(id).into())
        } else {
            let object = Resident::new(object);
            let lease = object.lease(id)?;
            self.objects.insert(id, object.into_any());
            //Dispatch::init(&mut lease, self)?;
            Ok(lease)
        }
    }
    /// Create a resident object that has discarded concrete type information
    #[inline]
    pub fn reserve<T: 'static + Dispatch>(object: T) -> Resident<dyn Any> {
        Resident::new(object).into_any()
    }
    /// Attempt to insert an object that has been reserved as a resident
    pub fn insert_any(&mut self, id: impl Object, object: Resident<dyn Any>) -> Result<Lease<dyn Any>> {
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
    /// Register a timer event with the client
    /// # Panics
    /// If called from another timer event
    pub fn register_timer(&mut self, event: TimerEvent) {
        self.timer_events.as_mut().expect("Cannot register a timer within a timer signal handler").add(event)
    }
    /// Register an event with the client. Prefer a TimerEvent where possible.
    /// # Panics
    /// If called from another event
    pub fn register_event(&mut self, event: Box<dyn Event>) {
        self.generic_events.as_mut().expect("Cannot register an event within another event signal handler").add(event)
    }
}

/// A generic event source
pub trait Event {
    /// Indicate if the event is ready to be signalled
    fn poll(&mut self) -> bool;
    /// Called when the event is ready
    /// 
    /// Return true if the event should stay registered
    fn signal(&mut self, client: &mut Client) -> Result<bool>;
}
/// A singly-linked list of events stored in no particular order
pub enum EventList {
    Tail,
    Event {
        event: Box<dyn Event>,
        next: Box<Self>
    }
}
impl EventList {
    pub fn new() -> Self {
        Self::Tail
    }
    /// Add an event to the front of the list
    pub fn add(&mut self, event: Box<dyn Event>) {
        // Take the old list, replacing self with an empty list
        let mut old = Self::Tail;
        std::mem::swap(self, &mut old);
        // Move the old list with the prepended element back in
        *self = Self::Event {
            event,
            next: Box::new(old)
        };
    }
    pub fn signal_ready(&mut self, client: &mut Client) -> Result<()> {
        let mut node = self;
        while let Self::Event { event, next } = node {
            if event.poll() {
                if !event.signal(client)? {
                    // Take the original list minus the head
                    let mut head = Self::new();
                    std::mem::swap(&mut head, next);
                    // Swap the lists such that the one containing just the head is in `head`
                    std::mem::swap(&mut head, node);
                    // The head element is now owned and pruned off
                } else {
                    // Safety: The lifetime is valid, the current version of borrow check fails to handle this correctly
                    // Yes, I hate doing this, but the alternatives are less than ideal
                    // Compilation with rustc flag `-Z polonius` allows this to succeed:
                    // node = next
                    node = unsafe { &mut *(next.deref_mut() as *mut _)}
                }
            } else {
                node = unsafe { &mut *(next.deref_mut() as *mut _)}
            }
        }
        Ok(())
    }
}
impl Default for EventList {
    fn default() -> Self {
        Self::Tail
    }
}
/// An event that will be signaled after a time
pub struct TimerEvent {
    creation: Instant,
    duration: Duration,
    pub signal: Box<dyn FnOnce(&mut Client) -> Result<()>>
}
impl TimerEvent {
    /// Create a new timer to be signalled after the duration has passed
    pub fn new<F: 'static + FnOnce(&mut Client) -> Result<()>>(duration: Duration, signal: F) -> Self {
        Self {
            creation: Instant::now(),
            duration,
            signal: Box::new(signal)
        }
    }
}
/// A singly-linked list of timer events ordered chronologically with events to fire sooner being placed at the head of the list
/// 
/// ```rust
/// # use std::{time::*, rc::Rc, cell::RefCell};
/// # use wl::server::{TimerEvent, TimerList};
/// let mut list = TimerList::new();
/// let mut output = Rc::new(RefCell::new(Vec::<u8>::new()));
/// 
/// let mut o = output.clone();
/// list.insert(TimerEvent::new(Duration::from_millis(200), move |client| Ok({o.borrow_mut().push(0);})));
/// let mut o = output.clone();
/// list.insert(TimerEvent::new(Duration::from_millis(250), move |client| Ok({o.borrow_mut().push(1);})));
/// let mut o = output.clone();
/// list.insert(TimerEvent::new(Duration::from_millis(50),  move |client| Ok({o.borrow_mut().push(2);})));
/// let mut o = output.clone();
/// list.insert(TimerEvent::new(Duration::from_millis(250), move |client| Ok({o.borrow_mut().push(3);})));
/// 
/// # #[allow(deref_nullptr)] let client = unsafe { &mut *std::ptr::null_mut() }; // Technically UB, need better stub
/// std::thread::sleep(Duration::from_millis(300));
/// list.signal_ready(client).unwrap();
/// 
/// assert_eq!(vec![2, 0, 1, 3], *output.borrow());
/// ```
pub enum TimerList {
    Tail,
    Event {
        timer: TimerEvent,
        next: Box<TimerList>
    }
}
impl TimerList {
    /// Create a new, empty, list
    pub fn new() -> Self {
        Self::Tail
    }
    /// Creates an iterator over each timer event
    pub fn iter(&self) -> TimerListIter {
        TimerListIter { list: self }
    }
    /// Insert the event in the list chronologically
    pub fn add(&mut self, mut event: TimerEvent) {
        let mut node = self;
        while let Self::Event { timer, next } = node {
            let fires_later = (event.creation + event.duration) > (timer.creation + timer.duration);
            if fires_later {
                node = next;
            } else {
                // Split the list
                let mut new = Box::new(Self::new());
                std::mem::swap(&mut new, next);
                // Insert the earlier event at the end of the other list
                std::mem::swap(&mut event, timer);
                // Merge the end of the list back on
                **next = TimerList::Event {
                    timer: event,
                    next: new
                };
                return
            }
        }
        // Append to the end
        let new = Self::Event {
            timer: event,
            next: Box::new(Self::Tail)
        };
        *node = new;
    }
    /// Signals all ready timers, removing them from the list
    pub fn signal_ready(&mut self, client: &mut Client) -> Result<()> {
        let now = std::time::Instant::now();
        while let Self::Event { timer, next } = self {
            let is_ready = (timer.creation + timer.duration) < now;
            if is_ready {
                // Take the original list minus the head
                let mut head = Self::new();
                std::mem::swap(&mut head, next);
                // Swap the lists such that the one containing just the head is in `head`
                std::mem::swap(&mut head, self);
                // The head element is now owned and pruned off
                if let TimerList::Event { timer, ..} = head {
                    (timer.signal)(client)?
                }
            } else {
                break
            }
        }
        Ok(())
    }
}
impl Default for TimerList {
    fn default() -> Self {
        Self::Tail
    }
}
pub struct TimerListIter<'a> {
    list: &'a TimerList
}
impl<'a> Iterator for TimerListIter<'a> {
    type Item = &'a TimerEvent;

    fn next(&mut self) -> Option<Self::Item> {
        if let TimerList::Event { timer, next } = self.list {
            self.list = next;
            Some(timer)
        } else {
            None
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
    fn new(value: T) -> Self {
        Self {
            ptr: Box::leak(Box::new(LeaseBox { leased: false, interface: T::INTERFACE, version: T::VERSION, dispatch: T::dispatch, value }))
        }
    }
}
impl<T: 'static + Any> Resident<T> {
    fn into_any(self) -> Resident<dyn Any> {
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
                Box::from_raw(self.ptr);
            }
        }
    }
}

/// A temporary claim of ownership of an object. Ownership will be returned implicitly to the corresponding `Resident` on drop.
pub struct Lease<T: ?Sized> {
    ptr: *mut LeaseBox<T>,
    id: u32
}
impl<T: Dispatch> Lease<T> {
    /// Creates a lease that will never have a corresponding resident
    fn temporary(id: u32, value: T) -> Self {
        Self {
            ptr: Box::leak(Box::new(LeaseBox { leased: false, interface: T::INTERFACE, version: T::VERSION, dispatch: T::dispatch, value })),
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