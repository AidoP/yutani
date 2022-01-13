use std::{fmt::{self, Display}, ops::{Deref, DerefMut}};

use crate::common::*;

/// A fixed-point decimal type as defined for the Wayland protocol
// TODO: -0 != 0, float conversion and impl Ord
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Fixed(pub i32);
impl Fixed {
    fn into_f32(self) -> f32 {
        todo!()
    }
}
impl From<i32> for Fixed {
    fn from(int: i32) -> Self {
        Self(int * 256)
    }
}
impl From<f32> for Fixed {
    fn from(_: f32) -> Self {
        todo!(/* Is f32 guaranteed to be an IEEE 754 single precision */)
    }
}
impl From<f64> for Fixed {
    fn from(_: f64) -> Self {
        todo!(/* Is f64 guaranteed to be an IEEE 754 double precision */)
    }
}
impl From<Fixed> for f32 {
    fn from(f: Fixed) -> f32 {
        todo!(/* Is f32 guaranteed to be an IEEE 754 single precision */)
    }
}
impl Into<f64> for Fixed {
    fn into(self) -> f64 {
        todo!(/* Is f64 guaranteed to be an IEEE 754 double precision */)
    }
}
impl Into<i32> for Fixed {
    fn into(self) -> i32 {
        self.0 / 256
    }
}
impl Display for Fixed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.into_f32())
    }
}

/// An id for an object that was requested to be created
/// 
/// Generally the interface of the new object is known ahead of time
/// thanks to the agreed upon protocol, however in some instances a NewId may be generic.
/// In such cases, the name of the interface to instrantiate an object for is passed along side
#[derive(Debug)]
pub struct NewId {
    pub id: u32,
    pub version: u32,
    pub interface: String
}
impl NewId {
    pub fn new(id: u32, version: u32, interface: String) -> Self {
        Self {
            id,
            version,
            interface
        }
    }
}
impl Display for NewId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "new id [{} v{}]@{}", self.interface, self.version, self.id)
    }
}
impl PartialEq<str> for NewId {
    fn eq(&self, other: &str) -> bool {
        self.interface == other
    }
}
impl PartialEq<String> for NewId {
    fn eq(&self, other: &String) -> bool {
        self.interface.eq(other)
    }
}
impl Object for NewId {
    fn object(&self) -> u32 {
        self.id
    }
}

#[derive(Debug)]
pub struct Fd(i32);
impl Fd {
    pub fn new(fd: i32) -> Self {
        Self(fd)
    }
}
impl From<i32> for Fd {
    fn from(fd: i32) -> Self {
        Self(fd)
    }
}
impl Into<i32> for Fd {
    fn into(self) -> i32 {
        self.0
    }
}
impl Deref for Fd {
    type Target = i32;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Display for Fd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fd {}", self.0)
    }
}

#[repr(transparent)]
pub struct Array(pub Vec<u8>);
impl Deref for Array {
    type Target = Vec<u8>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for Array {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Display for Array {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "array ({}b)", self.len())
    }
}