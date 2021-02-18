/// A fixed-point decimal type as defined for the Wayland protocol
/// TODO: -0 != 0, float conversion and impl Ord
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Fixed(pub i32);
impl From<i32> for Fixed {
    fn from(int: i32) -> Self {
        Self(int * 256)
    }
}
impl From<f32> for Fixed {
    fn from(int: f32) -> Self {
        unimplemented!(/* Is f32 guaranteed to be an IEEE 754 single precision */)
    }
}
impl From<f64> for Fixed {
    fn from(int: f64) -> Self {
        unimplemented!(/* Is f64 guaranteed to be an IEEE 754 double precision */)
    }
}
impl Into<f32> for Fixed {
    fn into(self) -> f32 {
        unimplemented!(/* Is f32 guaranteed to be an IEEE 754 single precision */)
    }
}
impl Into<f64> for Fixed {
    fn into(self) -> f64 {
        unimplemented!(/* Is f64 guaranteed to be an IEEE 754 double precision */)
    }
}
impl Into<i32> for Fixed {
    fn into(self) -> i32 {
        self.0 / 256
    }
}

/// An id for an object that was requested to be created
/// Generally the interface of the new object is known ahead of time
/// thanks to the agreed upon protocol, however in some instances a NewId may be generic
/// In such cases the name of the interface to instrantiate an object for is passed along side
pub struct NewId<'a> {
    pub id: u32,
    pub interface: &'a str
}
impl<'a> NewId<'a> {
    pub fn new(id: u32, interface: &'a str) -> Self {
        Self {
            id, 
            interface
        }
    }
}
impl<'a> PartialEq<str> for NewId<'a> {
    fn eq(&self, other: &str) -> bool {
        self.interface == other
    }
}
impl<'a> PartialEq<String> for NewId<'a> {
    fn eq(&self, other: &String) -> bool {
        self.interface == other
    }
}