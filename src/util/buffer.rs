/// Wraps a block of bytes and provides an interface for reading/writing integers and byte slices.
///
/// The inner type `<T>` be a `Vec[u8]` a slice `&[u8]` or a mutable slice `&mut [u8]`.
///
/// Methods for reading data are provided for all inner object types, and for vectors and mutable slices
/// methods are also available for writing into the buffer.
///
/// Reading from and writing to the buffer can either be at an absolute offset passed or
/// at the current offset. When using the current offset methods, the current offset will
/// be advanced by the size of the object read or written.
///
/// The default endian ordering for integers read from or written to the buffer is the native
/// ordering of the system. Use `self.big_endian()` or `self.little_endian()` to set a specific
/// byte ordering.
pub struct ByteBuffer<T> {
    /// Byte-order of integers stored in this buffer
    endian: Endian,
    /// Current offset for reading or writing.
    offset: usize,
    /// The block of bytes wrapped by this buffer
    inner: T,
}

impl <T: AsMut<[u8]>> ByteBuffer<T> {

    /// Return a mutable slice of length `len` starting at `offset` into the buffer.
    pub fn mut_at(&mut self, offset: usize, len: usize) -> &mut [u8] {
        &mut self.inner.as_mut()[offset..offset+len]
    }

    /// Write an integer or a `&[u8]` slice at the specified `offset` into the buffer.
    ///
    /// For integers, the type may be any of: u8, u16, u32, u64
    ///
    pub fn write_at<V: Writeable>(&mut self, offset: usize, val: V) -> &mut Self {
        let sz = val.size();
        let endian = self.endian;
        val.write(self.mut_at(offset, sz), endian);
        self
    }

}

impl <T: AsRef<[u8]>> ByteBuffer<T> {

    /// Return a slice of length `len` starting at `offset` into the buffer.
    ///
    /// # Panics
    ///
    /// Panics if `offset + len` exceeds size of buffer.
    ///
    pub fn ref_at(&self, offset: usize, len: usize) -> &[u8] {
        &self.inner.as_ref()[offset..offset+len]
    }

    pub fn as_ref(&self) -> &[u8] {
        &self.inner.as_ref()
    }

    /// Read and return an integer value from the current offset and increment
    /// the current offset by the byte size of the integer type.
    ///
    /// The integer type `V` may be any of: u8, u16, u32, u64
    ///
    /// # Panics
    ///
    /// Panics if byte size of integer type added to current offset exceeds size
    /// of buffer.
    ///
    /// # Examples
    /// ```
    /// use ph::util::ByteBuffer;
    ///
    /// let bytes = &[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
    /// let mut buffer = ByteBuffer::from_bytes(bytes).big_endian();
    ///
    /// let n16 = buffer.read::<u16>();
    /// let n32: u32 = buffer.read();
    ///
    /// assert_eq!(n16, 0xAABB);
    /// assert_eq!(n32, 0xCCDDEEFF);
    ///
    /// ```
    pub fn read<V: Readable>(&mut self) -> V {
        let offset = self.offset;
        self.offset += V::SIZE;
        self.read_at(offset)
    }

    /// Read and return an integer value from the specified `offset` into the buffer.
    ///
    /// The integer type `V` may be any of: u8, u16, u32, u64
    ///
    /// # Panics
    ///
    /// Panics if byte size of integer type added to `offset` exceeds size
    /// of buffer.
    ///
    /// # Examples
    /// ```
    /// use ph::util::ByteBuffer;
    ///
    /// let bytes = &[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
    /// let mut buffer = ByteBuffer::from_bytes(bytes).big_endian();
    ///
    /// let n8 = buffer.read_at::<u8>(5);
    /// let n16: u16 = buffer.read_at(2);
    /// let n32: u32 = buffer.read_at(0);
    ///
    /// assert_eq!(n8, 0xFF);
    /// assert_eq!(n16, 0xCCDD);
    /// assert_eq!(n32, 0xAABBCCDD);
    /// ```
    ///
    pub fn read_at<V: Readable>(&self, offset: usize) -> V {
        let endian = self.endian;
        V::read(self.ref_at(offset, V::SIZE), endian)
    }

    /// Copy from the current offset into the slice `bytes` and increment the current
    /// offset by the size of `bytes`
    ///
    /// # Panics
    ///
    /// Panics if `bytes.len()` added to current offset exceeds size of buffer.
    ///
    pub fn read_bytes(&mut self, bytes: &mut [u8]) {
        let offset = self.offset;
        self.offset += bytes.len();
        self.read_bytes_at(offset, bytes);
    }

    /// Copy from the specified offset into the slice `bytes`
    ///
    /// # Panics
    ///
    /// Panics if `bytes.len() + offset` exceeds size of buffer.
    ///
    pub fn read_bytes_at(&self, offset: usize, bytes: &mut [u8]) {
        bytes.copy_from_slice(self.ref_at(offset, bytes.len()));
    }
}

impl <T> ByteBuffer<T> {
    fn new_with(inner: T) -> Self {
        ByteBuffer {
            endian: Endian::Native,
            offset: 0,
            inner,
        }
    }

    /// Set the current offset into the buffer to the value `offset`
    ///
    /// # Examples
    ///
    /// ```
    /// use ph::util::ByteBuffer;
    ///
    /// let mut buffer = ByteBuffer::from_bytes(&[0xAA, 0xBB, 0xCC, 0xDD]).big_endian();
    ///
    /// buffer.set_offset(2);
    /// let n: u8 = buffer.read();
    /// assert_eq!(n, 0xCC);
    ///
    /// buffer.set_offset(1);
    /// let n: u16 = buffer.read();
    /// assert_eq!(n, 0xBBCC);
    ///
    /// ```
    pub fn set_offset(&mut self, offset: usize) {
        self.offset = offset;
    }

    /// Configure this `ByteBuffer` instance to write integers in big-endian byte order
    ///
    /// Caller must chain this to call to constructor because it consumes and returns
    /// `self` argument.
    ///
    /// # Examples
    ///
    /// ```
    /// use ph::util::ByteBuffer;
    ///
    /// let mut buffer = ByteBuffer::from_bytes(&[0xAA, 0xBB, 0xCC, 0xDD])
    ///                 .big_endian();
    ///
    /// let n: u32 = buffer.read();
    ///
    /// assert_eq!(n, 0xAABBCCDD);
    /// ```
    ///
    pub fn big_endian(mut self) -> Self {
        self.endian = Endian::Big;
        self
    }

    /// Configure this `ByteBuffer` instance to write integers in little-endian byte order
    ///
    /// Caller must chain this to call to constructor because it consumes and returns
    /// `self` argument.
    ///
    /// # Examples
    ///
    /// ```
    /// use ph::util::ByteBuffer;
    ///
    /// let mut buffer = ByteBuffer::from_bytes(&[0xAA, 0xBB, 0xCC, 0xDD])
    ///                 .little_endian();
    ///
    /// let n: u32 = buffer.read();
    /// assert_eq!(n, 0xDDCCBBAA);
    ///
    /// let n: u16 = buffer.read_at(2);
    /// assert_eq!(n, 0xDDCC);
    /// ```
    ///
    pub fn little_endian(mut self) -> Self {
        self.endian = Endian::Little;
        self
    }
}

impl <'a> ByteBuffer<&'a [u8]> {
    /// Create a new read-only `ByteBuffer` from the slice `bytes`
    pub fn from_bytes(bytes: &'a [u8]) -> Self {
        ByteBuffer::new_with(bytes)
    }

    /// Return the byte length of the inner slice;
    pub fn len(&self) -> usize {
        self.inner.len()
    }
}

impl <'a> ByteBuffer<&'a mut [u8]> {
    /// Create a new `ByteBuffer` from the mutable slice `bytes`
    pub fn from_bytes_mut(bytes: &'a mut [u8]) -> Self {
        ByteBuffer::new_with(bytes)
    }

    /// Write an integer or a `&[u8]` slice at the current offset and increment
    /// the current offset by the size of `val`.
    ///
    /// For integers, the type may be any of: u8, u16, u32, u64
    ///
    pub fn write<V: Writeable>(&mut self, val: V) -> &mut Self {
        let offset = self.offset;
        self.offset += val.size();
        self.write_at(offset, val)
    }

    /// Return the byte length of the inner slice;
    pub fn len(&self) -> usize {
        self.inner.len()
    }
}

impl ByteBuffer<Vec<u8>> {
    /// Create a `size` length byte buffer and initialize the entire buffer with
    /// `0u8` (zero bytes).
    pub fn new(size: usize) -> Self {
        Self::from_vec(vec![0u8; size])
    }

    /// Create an empty buffer (`self.len() == 0`) with an inner vector instance.
    ///
    /// Data can be appended to this buffer with `self.write()`
    ///
    pub fn new_empty() -> Self {
        Self::from_vec(Vec::new())
    }

    /// Create a buffer from a `Vec<u8>`
    pub fn from_vec(vec: Vec<u8>) -> Self {
        Self::new_with(vec)
    }

    /// Returns the byte length of the inner vector.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Write an integer or a `&[u8]` slice at the current offset and increment
    /// the current offset by the size of `val`.
    ///
    /// For integers, the type may be any of: u8, u16, u32, u64
    ///
    /// If the size of the integer type added to the current offset exceeds
    /// the length of the vector, the vector will be resized.
    ///
    /// # Examples
    /// ```
    /// use ph::util::ByteBuffer;
    ///
    /// let mut buf = ByteBuffer::new_empty().big_endian();
    ///
    /// assert_eq!(buf.len(), 0);
    ///
    /// let n: u32 = 0xAABBCCDD;
    ///
    /// buf.write(n);
    ///
    /// assert_eq!(buf.as_ref(), &[0xAA, 0xBB, 0xCC, 0xDD]);
    ///
    /// buf.write(n);
    ///
    /// assert_eq!(buf.len(), 8);
    ///
    /// ```
    pub fn write<V: Writeable>(&mut self, val: V) -> &mut Self {
        let offset = self.offset;
        self.offset += val.size();
        if self.offset > self.inner.len() {
            self.inner.resize(self.offset, 0);
        }
        self.write_at(offset, val)
    }
}

/// The byte-order configuration of a `ByteBuffer`
#[derive(Copy,Clone,Debug)]
pub enum Endian {
    Big,
    Little,
    Native,
}

/// An object type which can be read from a `ByteBuffer` with the
/// `self.read()` or `self.read_at()` methods.
pub trait Readable {
    const SIZE: usize;
    fn read(bytes: &[u8], endian: Endian) -> Self;
}

/// An object type which can be written to a `ByteBuffer` with the
/// `self.write(val)` or `self.write_at(val)` methods.
pub trait Writeable {
    fn size(&self) -> usize;
    fn write(&self, bytes: &mut [u8], endian: Endian);
}

impl Writeable for &[u8] {
    fn size(&self) -> usize {
        self.len()
    }
    fn write(&self, bytes: &mut [u8], _endian: Endian) {
        bytes.copy_from_slice(self);
    }
}

macro_rules! storeable_int {
    {$T:ty} => {
        impl Writeable for $T {
            fn size(&self) -> usize {
                ::std::mem::size_of::<$T>()
            }
            fn write(&self, bytes: &mut [u8], endian: Endian) {
                bytes.copy_from_slice(&match endian {
                    Endian::Big    => self.to_be_bytes(),
                    Endian::Little => self.to_le_bytes(),
                    Endian::Native => self.to_ne_bytes(),
                });
            }
        }

        impl Readable for $T {
            const SIZE: usize = ::std::mem::size_of::<$T>();

            fn read(bytes: &[u8], endian: Endian) -> Self {
                let mut buf = [0u8; Self::SIZE];
                buf.copy_from_slice(&bytes[..Self::SIZE]);
                match endian {
                    Endian::Big => <$T>::from_be_bytes(buf),
                    Endian::Little=> <$T>::from_le_bytes(buf),
                    Endian::Native=> <$T>::from_ne_bytes(buf),
                }
            }
        }
    }
}

storeable_int!(u8);
storeable_int!(u16);
storeable_int!(u32);
storeable_int!(u64);
