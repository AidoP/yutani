use crate::common::*;

/// A message over the wire
/// Each message refers to an action to carry out on an Object with given arguments defined by the interface that the object implements
/// Messages are decoded and encoded by `#[interface("protocol")]` attributes and should not be used otherwise
#[derive(Debug)]
pub struct Message {
    /// The object instance that the message refers to
    pub object: u32,
    /// The event or request to carry out
    pub opcode: u16,
    /// The untyped arguments to pass to the callee
    pub args: Vec<u32>,
    /// The file descriptor arguments
    pub fds: Vec<Fd>
}
impl Message {
    /// Create a new message with no arguments
    pub fn new(object: u32, opcode: u16) -> Self {
        Self {
            object,
            opcode,
            args: vec![],
            fds: vec![]
        }
    }
    /// Peek to see if a full message is available on the ring buffer
    pub fn available(messages: &RingBuffer) -> bool {
        if let Ok([_, _, _, _, _, _, a, b]) = messages.copy() {
            messages.len() >= u16::from_ne_bytes([a, b]) as usize
        } else {
            false
        }
    }
    /// Decode the next message directly off the wire
    pub fn read(messages: &mut RingBuffer) -> Result<Self> {
        let object = u32::from_ne_bytes(messages.take()?);
        let p = u32::from_ne_bytes(messages.take()?);
        let message_size = (p >> 16) as u16;
        let opcode = p as u16;

        if message_size & 0b11 != 0 || message_size < 8 {
            return Err(DispatchError::IOError(std::io::ErrorKind::InvalidData.into()))
        }
        // TODO: A vec with a small stack buffer will see a large speed increase
        let mut args = vec![0; (message_size as usize / std::mem::size_of::<u32>()) - 2];
        unsafe { messages.take_into_raw(args.as_mut_ptr() as *mut u8, args.len() * std::mem::size_of::<u32>())? };

        Ok(Self {
            object,
            opcode,
            args,
            fds: Vec::new()
        })
    }
    /// Send the message along the wire for a given interface version
    pub fn send(mut self, stream: &mut UnixStream) -> Result<()> {
        let args_size = self.args.len() * std::mem::size_of::<u32>();
        let message_size = 8 + args_size;
        let info = (message_size << 16) as u32 | self.opcode as u32;

        stream.sendmsg(&mut [
            IoVec::from(self.object.to_ne_bytes().as_mut_slice()),
            IoVec::from(info.to_ne_bytes().as_mut_slice()),
            IoVec::from(unsafe { std::slice::from_raw_parts_mut(self.args.as_mut_ptr() as *mut u8, args_size) })
        ], &self.fds)?;
        Ok(())
    }
    /// Get an adapter over the arguments
    pub fn args<'a>(&'a self) -> Args<'a> {
        Args {
            args: &self.args
        }
    }
    /// Push a u32 to the list of arguments
    pub fn push_u32(&mut self, int: u32) {
        self.args.push(int)
    }
    /// Push a i32 to the list of arguments
    pub fn push_i32(&mut self, int: i32) {
        self.args.push(int as _)
    }
    /// Push a Fixed to the list of arguments
    pub fn push_fixed(&mut self, fixed: Fixed) {
        self.args.push(fixed.0 as u32)
    }
    /// Push a string to the list of arguments, appending a null-terminator
    /// Use `push_bytes()` if you are pushing a string that is already null-terminated
    pub fn push_str<Bytes: AsRef<[u8]>>(&mut self, str: Bytes) {
        let chunks = str.as_ref().chunks_exact(std::mem::size_of::<u32>());
        let r = chunks.remainder();
        // As we add a character and div rounds down, we always add an extra u32 to the length 
        self.args.push(str.as_ref().len() as u32 + 1);
        self.args.extend(chunks.map(|b| u32::from_ne_bytes([b[0], b[1], b[2], b[3]])));
        self.args.push(
            match r.len() {
                0 => 0,
                1 => u32::from_ne_bytes([r[0], b'\0', 0, 0]),
                2 => u32::from_ne_bytes([r[0], r[1], b'\0', 0]),
                3 => u32::from_ne_bytes([r[0], r[1], r[2], b'\0']),
                _ => unreachable!()
            }
        )
    }
    /// Push an array of bytes to the list of arguments
    pub fn push_array(&mut self, array: Array) {
        let chunks = array.as_slice().chunks_exact(std::mem::size_of::<u32>());
        let r = chunks.remainder();
        self.args.push(array.as_slice().len() as u32 | 0b11);
        self.args.extend(chunks.map(|b| u32::from_ne_bytes([b[0], b[1], b[2], b[3]])));
        match r.len() {
            0 => (),
            1 => self.args.push(u32::from_ne_bytes([r[0], 0, 0, 0])),
            2 => self.args.push(u32::from_ne_bytes([r[0], r[1], 0, 0])),
            3 => self.args.push(u32::from_ne_bytes([r[0], r[1], r[2], 0])),
            _ => unreachable!()
        }
    }
    /// Push a file to the list of arguments
    pub fn push_file(&mut self, fd: Fd) {
        self.fds.push(fd)
    }
    /// Push a u32 to the list of arguments
    pub fn push_new_id(&mut self, id: NewId) {
        self.push_u32(id.id)
    }
    /// Push a u32 to the list of arguments
    pub fn push_dynamic_new_id(&mut self, id: NewId) {
        self.push_str(id.interface);
        self.push_u32(id.version);
        self.push_u32(id.id)
    }
}

/// An adapter over a &[u32] stream for parsing arguments
/// Each access consumes u32's from the stream
pub struct Args<'a> {
    args: &'a [u32]
}
impl<'a> Args<'a> {
    /// Interpret the next argument as an unsigned integer
    pub fn next_u32(&mut self) -> Result<u32> {
        self.args.first().map(|&i| {
            self.args = &self.args[1..];
            i
        }).ok_or(DispatchError::ExpectedArgument("uint"))
    }
    /// Interpret the next argument as a signed integer
    pub fn next_i32(&mut self) -> Result<i32> {
        self.args.first().map(|&i| {
            self.args = &self.args[1..];
            i as i32
        }).ok_or(DispatchError::ExpectedArgument("int"))
    }
    /// Interpret the next argument as a Fixed-point decimal
    pub fn next_fixed(&mut self) -> Result<Fixed> {
        self.next_i32().map(|i| Fixed(i))
    }
    /// Interpret the next argument as a byte string
    /// TODO: Should we be doing a lossy conversion? (Just use an array if it isn't utf8...)
    pub fn next_str(&mut self) -> Result<String> {
        let mut len = self.next_u32()? as usize;
        // Round up to the next aligned index
        if len & 0b11 != 0 {
            len = (len & !0b11) + 4;
        }
        if len > self.args.len() * std::mem::size_of::<u32>() {
            Err(DispatchError::ExpectedArgument("string"))
        } else {
            // Transmute to a &[u8], careful to update the length to be in the correct units and to keep the same lifetime
            let str: &'a [u8] = unsafe { std::slice::from_raw_parts(self.args.as_ptr() as *const u8, self.args.len() * std::mem::size_of::<u32>() / std::mem::size_of::<u8>()) };
            self.args = &self.args[len / std::mem::size_of::<u32>()..];
            // TODO: Should we trust the length? Are nulls in a &str potentially hazardous?
            let null_index = str[..len].iter().take_while(|&&b| b != 0).count(); // Too lenient
            Ok(String::from_utf8_lossy(&str[..null_index]).to_string())
        }

    }
    // TODO: Transmute to useful types with generic implementation. Can it be done safely?
    /// Interpret the next argument as a byte slice
    /// Similar to `next_str()` but can contain null bytes
    pub fn next_array(&mut self) -> Result<&'a [u8]> {
        let len = self.next_u32()? as usize;
        // Round up to the next aligned index
        let aligned_len = if len & 0b11 != 0 {
            (len & !0b11) + 4
        } else {
            len
        };
        if self.args.len() * std::mem::size_of::<u32>() < aligned_len {
            Err(DispatchError::ExpectedArgument("array"))
        } else {
            // TODO: Don't trust user input
            // Transmute to a &[u8], careful to update the length to be in the correct units and to keep the same lifetime
            let array: &'a [u8] = unsafe { std::slice::from_raw_parts(self.args.as_ptr() as *const u8, len) };
            self.args = &self.args[aligned_len / std::mem::size_of::<u32>()..];
            Ok(array)
        }
    }
    /// Interpret the next argument as a new_id where the type is known through the protocol specification
    pub fn next_new_id<S: Into<String>>(&mut self, interface: S, version: u32) -> Result<NewId> {
        let id = self.next_u32().map_err(|_| DispatchError::ExpectedArgument("new_id id"))?;
        Ok(NewId {
            id,
            interface: interface.into(),
            version
        })
    }
    /// Interpret the next argument as a new_id of which the type is unknown statically
    pub fn next_dynamic_new_id(&mut self) -> Result<NewId> {
        let interface = self.next_str().map_err(|_| DispatchError::ExpectedArgument("new_id interface"))?;
        let version = self.next_u32().map_err(|_| DispatchError::ExpectedArgument("new_id version"))?;
        let id = self.next_u32().map_err(|_| DispatchError::ExpectedArgument("new_id id"))?;
        Ok(NewId {
            id,
            interface,
            version
        })
    }
}