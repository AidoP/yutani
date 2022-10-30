use std::{ptr::NonNull, ops::{Deref, DerefMut}, any::Any};

use crate::prelude::*;

pub trait Object<T> {
    
}

#[derive(Debug)]
pub struct DispatchError {
    object: Id,
    error: u32
}

pub type DispatchFn<S, C> = fn(Lease<dyn Any>, &mut EventLoop<S>, &mut C) -> std::result::Result<(), DispatchError>;

struct RawLease<T: ?Sized> {
    leased: bool,
    id: Id,
    interface: &'static str,
    version: u32,
    value: T
}
/// An object that maintains ownership that can be leased out. Together, `Resident` and `Lease` provide an
/// asymmetric ownership model, which allow for mutable access to what would otherwise be owned data.
/// 
/// Only one `Lease` may exist at a time, and while it exists access to the mutable contents is prohibited to
/// uphold the guarantee that a mutable reference provides. Immutable fields are also present on `Resident` and
/// `Lease` which describe it as an object under the Wayland protocol.
/// 
/// The relationship between `Resident` and `Lease` is similar to that of
/// `Rc` and `Weak`, where `Resident` 
pub struct Resident<T: ?Sized, S, C> {
    dispatch: DispatchFn<S ,C>,
    lease: NonNull<RawLease<T>>
}
impl<T, S, C> Resident<T, S, C> {
    pub fn new(id: Id, dispatch: DispatchFn<S, C>, interface: &'static str, version: u32, value: T) -> Self {
        let boxed = Box::new(RawLease {
            leased: false,
            id,
            interface,
            version,
            value
        });
        Self {
            dispatch,
            lease: unsafe { NonNull::new_unchecked(Box::leak(boxed)) }
        }
    }
}
impl<T: Any, S, C> Resident<T, S, C> {
    pub fn into_any(self) -> Resident<dyn Any, S, C> {
        let this: Resident<dyn Any, S, C> = Resident {
            dispatch: self.dispatch,
            lease: self.lease
        };
        // Ensure the old resident doesn't free the RawLease
        std::mem::forget(self);
        this
    }
}
impl<T: ?Sized, S, C> Resident<T, S, C> {
    pub fn get(&self) -> Option<&T> {
        if unsafe { self.lease.as_ref() }.leased {
            None
        } else {
            Some(&unsafe { self.lease.as_ref() }.value)
        }
    }
    pub fn get_mut(&mut self) -> Option<&mut T> {
        if unsafe { self.lease.as_ref() }.leased {
            None
        } else {
            Some(&mut unsafe { self.lease.as_mut() }.value)
        }
    }
    pub fn id(&self) -> Id {
        unsafe { self.lease.as_ref() }.id
    }
    pub fn interface(&self) -> &'static str {
        unsafe { self.lease.as_ref() }.interface
    }
    pub fn version(&self) -> u32 {
        unsafe { self.lease.as_ref() }.version
    }
    pub fn lease(&mut self) -> Result<Lease<T>> {
        if unsafe { self.lease.as_ref() }.leased {
            Err(Error::DoubleLease)
        } else {
            unsafe { self.lease.as_mut() }.leased = true;
            Ok(Lease(unsafe { NonNull::new_unchecked(self.lease.as_mut()) }))
        }
    }
}
impl<S, C> Resident<dyn Any, S, C> {
    /// # Panics
    /// Panics if there is already a lease.
    #[inline]
    pub fn dispatch(mut self, event_loop: &mut EventLoop<S>, client: &mut C) -> std::result::Result<(), DispatchError> {
        let dispatch = self.dispatch;
        let lease = self.lease().expect("Double lease");
        dispatch(lease, event_loop, client)
    }
}
impl<T: ?Sized, S, C> Drop for Resident<T, S, C> {
    fn drop(&mut self) {
        if !unsafe { self.lease.as_ref() }.leased {
            drop(unsafe { Box::from_raw(self.lease.as_ptr()) })
        } else {
            unsafe { self.lease.as_mut() }.leased = false;
        }
    }
}
#[repr(transparent)]
pub struct Lease<T: ?Sized>(NonNull<RawLease<T>>);
impl<T: Any> Lease<T> {
    pub fn into_any(self) -> Lease<dyn Any> {
        let lease: Lease<dyn Any> = Lease(self.0);
        // Ensure the old lease doesn't free the RawLease
        std::mem::forget(self);
        lease
    }
}
impl Lease<dyn Any> {
    pub fn downcast<T: Any>(self) -> Option<Lease<T>> {
        if unsafe { self.0.as_ref() }.value.is::<T>() {
            let lease = Some(Lease(unsafe { NonNull::new_unchecked(self.0.as_ptr().cast()) }));
            // Ensure the old lease doesn't free the RawLease
            std::mem::forget(self);
            lease
        } else {
            None
        }
    }
}
impl<T: ?Sized> Lease<T> {
    pub fn id(&self) -> Id {
        unsafe { self.0.as_ref() }.id
    }
    pub fn interface(&self) -> &'static str {
        unsafe { self.0.as_ref() }.interface
    }
    pub fn version(&self) -> u32 {
        unsafe { self.0.as_ref() }.version
    }
}
impl<T: ?Sized> Deref for Lease<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &unsafe { self.0.as_ref() }.value
    }
}
impl<T: ?Sized> DerefMut for Lease<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut unsafe { self.0.as_mut() }.value
    }
}
impl<T: ?Sized> Drop for Lease<T> {
    fn drop(&mut self) {
        if !unsafe { self.0.as_ref() }.leased {
            drop(unsafe { Box::from_raw(self.0.as_ptr()) })
        } else {
            unsafe { self.0.as_mut() }.leased = false;
        }
    }
}