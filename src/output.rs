use std::fmt::{self, Write as _};
use std::io::{self, Read, Seek, SeekFrom, Write};

use crate::{Limits, Primitive};

/// Serialization state shared by the object writers, independent of the sink.
#[doc(hidden)]
pub trait Output {
    fn extend(&mut self, bytes: &[u8]);
    fn len(&self) -> u64;
    fn last(&self) -> Option<u8>;
    fn limits(&self) -> &Limits;
    fn limits_mut(&mut self) -> &mut Limits;
}

/// Owns the destination and retains the first error, including errors from Drop.
pub(crate) struct Sink<W> {
    pub(crate) writer: W,
    len: u64,
    last: Option<u8>,
    limits: Limits,
    error: Option<io::Error>,
}

impl<W: Write> Sink<W> {
    pub(crate) fn new(writer: W) -> Self {
        Self {
            writer,
            len: 0,
            last: None,
            limits: Limits::new(),
            error: None,
        }
    }

    pub(crate) fn as_output(&mut self) -> &mut dyn Output {
        self
    }

    pub(crate) fn fail(&mut self, error: io::Error) {
        if self.error.is_none() {
            self.error = Some(error);
        }
    }

    pub(crate) fn failed(&self) -> bool {
        self.error.is_some()
    }

    pub(crate) fn finish(mut self) -> io::Result<W> {
        if let Some(error) = self.error {
            return Err(error);
        }
        self.writer.flush()?;
        Ok(self.writer)
    }
}

impl<W: Write + Read + Seek> Sink<W> {
    /// Flush before reading back, and restore the write position even on error.
    pub(crate) fn read<T>(
        &mut self,
        f: impl FnOnce(&mut W) -> io::Result<T>,
    ) -> io::Result<T> {
        if self.failed() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "cannot read a failed chunk",
            ));
        }
        self.writer.flush()?;
        let position = self.writer.stream_position()?;
        let result = f(&mut self.writer);
        let restored = self.writer.seek(SeekFrom::Start(position));
        let value = result?;
        restored?;
        Ok(value)
    }
}

impl<W: Write> Output for Sink<W> {
    fn extend(&mut self, bytes: &[u8]) {
        if self.failed() {
            return;
        }
        if let Err(error) = self.write_all(bytes) {
            self.fail(error);
        }
    }

    fn len(&self) -> u64 {
        self.len
    }
    fn last(&self) -> Option<u8> {
        self.last
    }
    fn limits(&self) -> &Limits {
        &self.limits
    }
    fn limits_mut(&mut self) -> &mut Limits {
        &mut self.limits
    }
}

impl<W: Write> Write for Sink<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let written = self.writer.write(bytes)?;
        self.len += written as u64;
        if let Some(&last) = bytes[..written].last() {
            self.last = Some(last);
        }
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

impl dyn Output + '_ {
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub(crate) fn push_val<T: Primitive>(&mut self, value: T) {
        value.write(self);
    }

    #[inline]
    pub(crate) fn push_int(&mut self, value: i32) {
        self.limits_mut().register_int(value);
        self.extend(itoa::Buffer::new().format(value).as_bytes());
    }

    #[inline]
    pub(crate) fn push_float(&mut self, value: f32) {
        if value as i32 as f32 == value {
            self.push_int(value as i32);
        } else {
            self.push_decimal(value);
        }
    }

    #[inline]
    pub(crate) fn push_decimal(&mut self, value: f32) {
        self.limits_mut().register_real(value);
        if value == 0.0 || (value.abs() > 1e-6 && value.abs() < 1e12) {
            self.extend(ryu::Buffer::new().format(value).as_bytes());
        } else {
            write!(self, "{value}").unwrap();
        }
    }

    #[inline]
    pub(crate) fn push(&mut self, byte: u8) {
        self.extend(&[byte]);
    }

    #[inline]
    pub(crate) fn push_hex(&mut self, value: u8) {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        self.extend(&[HEX[(value >> 4) as usize], HEX[(value & 15) as usize]]);
    }

    #[inline]
    pub(crate) fn push_hex_u16(&mut self, value: u16) {
        self.push_hex((value >> 8) as u8);
        self.push_hex(value as u8);
    }

    #[inline]
    pub(crate) fn push_octal(&mut self, value: u8) {
        self.extend(&[
            b'0' + (value >> 6),
            b'0' + ((value >> 3) & 7),
            b'0' + (value & 7),
        ]);
    }
}

// Formatting emits serialization tokens; I/O errors are retained by the sink.
impl fmt::Write for dyn Output + '_ {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.extend(text.as_bytes());
        Ok(())
    }
}
