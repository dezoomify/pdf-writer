use std::fmt::{self, Write as _};
use std::io::{self, Write};

use crate::{Limits, Primitive};

// Erase only the destination type so nested object writers stay nongeneric.
pub type Output<'a> = Sink<dyn Write + 'a>;

/// Serialization state and a caller-supplied destination.
#[doc(hidden)]
pub struct Sink<W: Write + ?Sized> {
    len: u64,
    last: Option<u8>,
    limits: Limits,
    error: Option<io::Error>,
    pub(crate) writer: W,
}

impl<W: Write> Sink<W> {
    pub(crate) fn new(writer: W) -> Self {
        Self {
            len: 0,
            last: None,
            limits: Limits::new(),
            error: None,
            writer,
        }
    }

    pub(crate) fn as_output(&mut self) -> &mut Output<'_> {
        self
    }

    pub(crate) fn flush(&mut self) -> io::Result<()> {
        self.check()?;
        if let Err(error) = self.writer.flush() {
            self.error = Some(error);
        }
        self.check()
    }

    pub(crate) fn finish(mut self) -> io::Result<W> {
        if let Some(error) = self.error {
            return Err(error);
        }
        self.writer.flush()?;
        Ok(self.writer)
    }
}

impl<W: Write + ?Sized> Sink<W> {
    pub(crate) fn fail(&mut self, error: io::Error) {
        if self.error.is_none() {
            self.error = Some(error);
        }
    }

    pub(crate) fn failed(&self) -> bool {
        self.error.is_some()
    }

    // Keep the original error for finish(), while allowing borrowed chunks to
    // report failure before their bytes are copied to another destination.
    pub(crate) fn check(&self) -> io::Result<()> {
        match &self.error {
            Some(error) => Err(io::Error::new(error.kind(), error.to_string())),
            None => Ok(()),
        }
    }

    pub(crate) fn len(&self) -> u64 {
        self.len
    }
    pub(crate) fn last(&self) -> Option<u8> {
        self.last
    }
    pub(crate) fn limits(&self) -> &Limits {
        &self.limits
    }
    pub(crate) fn limits_mut(&mut self) -> &mut Limits {
        &mut self.limits
    }

    pub(crate) fn extend(&mut self, mut bytes: &[u8]) {
        while !bytes.is_empty() && self.error.is_none() {
            match self.writer.write(bytes) {
                Ok(0) => self.error = Some(io::ErrorKind::WriteZero.into()),
                Ok(n) => {
                    self.len += n as u64;
                    self.last = Some(bytes[n - 1]);
                    bytes = &bytes[n..];
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => self.error = Some(error),
            }
        }
    }
}

impl Output<'_> {
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

// Formatting cannot fail independently; I/O errors are retained until finish.
impl<W: Write + ?Sized> fmt::Write for Sink<W> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.extend(text.as_bytes());
        Ok(())
    }
}
