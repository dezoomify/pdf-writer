use std::fs::File;
use std::io::{self, BufReader, BufWriter, Cursor, Read, Seek, SeekFrom, Write};

use crate::{Finish, Name, Pdf, PdfOptions, Ref};

/// A destination that can discard a partially written suffix.
pub trait RollbackWrite: Write {
    /// Preserve the prefix, truncate to `position`, and position the next write
    /// there. Discard or flush any buffered bytes before truncating. On failure
    /// the destination may be partially modified and must not be published.
    fn rollback_to(&mut self, position: u64) -> io::Result<()>;
}

impl RollbackWrite for File {
    fn rollback_to(&mut self, position: u64) -> io::Result<()> {
        self.set_len(position)?;
        self.seek(SeekFrom::Start(position))?;
        Ok(())
    }
}

impl RollbackWrite for Cursor<Vec<u8>> {
    fn rollback_to(&mut self, position: u64) -> io::Result<()> {
        if position > self.get_ref().len() as u64 {
            return Err(invalid("rollback position exceeds destination length"));
        }
        self.get_mut().truncate(position as usize);
        self.set_position(position);
        Ok(())
    }
}

impl<W: RollbackWrite> RollbackWrite for BufWriter<W> {
    fn rollback_to(&mut self, position: u64) -> io::Result<()> {
        self.flush()?;
        self.get_mut().rollback_to(position)
    }
}

/// A PDF with a caller-owned temporary file for object offsets.
///
/// Each [`write`](Self::write) call streams object bytes directly to the
/// destination, then spills its offsets to disk. RAM holds only the offsets
/// produced by that call. Keep calls small to bound memory. Finalization streams
/// an uncompressed cross-reference stream with a fixed-size buffer.
///
/// Optional transactions discard incomplete pages or other groups of objects,
/// including after a recoverable destination error. Object IDs remain the
/// caller's responsibility. A scratch-index failure is terminal.
///
/// ```
/// use std::{fs::File, io::BufWriter};
/// use pdf_writer::{FilePdf, Ref, Rect};
/// # fn example(output: File, scratch_index: File) -> std::io::Result<()> {
/// let mut pdf = FilePdf::new(BufWriter::new(output), scratch_index)?;
/// pdf.begin_transaction()?;
/// pdf.write(|pdf| {
///     pdf.page(Ref::new(3)).parent(Ref::new(2))
///         .media_box(Rect::new(0.0, 0.0, 100.0, 100.0)).resources();
/// })?;
/// pdf.commit_transaction()?;
/// pdf.write(|pdf| {
///     pdf.catalog(Ref::new(1)).pages(Ref::new(2));
///     pdf.pages(Ref::new(2)).kids([Ref::new(3)]).count(1);
/// })?;
/// pdf.finish(Ref::new(4))?;
/// # Ok(())
/// # }
/// ```
pub struct FilePdf<W: Write> {
    pdf: Pdf<W>,
    index: Index,
    transaction: Option<Transaction>,
    index_failed: bool,
}

struct Transaction {
    count: u64,
    max_id: i32,
}

impl<W: Write> FilePdf<W> {
    /// Both destinations must be empty, exclusively owned, and positioned at
    /// their beginning. `index` must be readable and writable. Its contents are
    /// scratch data, not a PDF, and its lifetime/cleanup belong to the caller.
    pub fn new(writer: W, index: File) -> io::Result<Self> {
        Self::with_options(writer, index, PdfOptions::default())
    }

    /// Create a disk-indexed PDF with header and object formatting options.
    /// Requires PDF 1.5 or later because finalization uses a cross-reference stream.
    /// See [`new`](Self::new) for destination requirements.
    pub fn with_options(writer: W, index: File, options: PdfOptions) -> io::Result<Self> {
        if options.version < (1, 5) {
            return Err(invalid("disk-indexed PDFs require PDF 1.5 or later"));
        }
        if index.metadata()?.len() != 0 {
            return Err(invalid("index must be empty"));
        }
        index.set_len(16)?;
        let pdf = Pdf::with_options(writer, options);
        pdf.check()?;
        Ok(Self {
            pdf,
            index: Index { file: index, count: 0, max_id: 0 },
            transaction: None,
            index_failed: false,
        })
    }

    /// Configure the destination without writing, seeking, or changing its
    /// contents. For example, disable a cooperative cancellation flag before
    /// rolling back. External byte changes invalidate object offsets.
    pub fn get_mut(&mut self) -> &mut W {
        self.pdf.get_mut()
    }

    /// Serialize a bounded group of objects, flush, and store their offsets.
    /// The callback must not replace the PDF, manage transactions, or modify its
    /// destination's bytes outside the object builders. Duplicate IDs and index
    /// errors are terminal.
    pub fn write(&mut self, write: impl FnOnce(&mut Pdf<W>)) -> io::Result<()> {
        self.pdf.check()?;
        write(&mut self.pdf);
        self.pdf.flush()?;
        for (id, offset) in std::mem::take(&mut self.pdf.chunk.offsets) {
            if let Err(error) = self.index.insert(id, offset) {
                self.index_failed = true;
                self.pdf.chunk.output.fail(error);
                return self.pdf.check();
            }
        }
        Ok(())
    }

    /// Begin one transaction at an object boundary. Nested transactions are
    /// rejected. Commit or roll back before beginning another or finishing.
    pub fn begin_transaction(&mut self) -> io::Result<()> {
        if self.transaction.is_some() {
            return Err(invalid("a transaction is already active"));
        }
        self.pdf.begin_transaction()?;
        self.transaction =
            Some(Transaction { count: self.index.count, max_id: self.index.max_id });
        Ok(())
    }

    /// Keep the transaction's objects. An earlier error leaves the transaction
    /// active so the caller can attempt rollback.
    pub fn commit_transaction(&mut self) -> io::Result<()> {
        self.pdf.commit_transaction()?;
        self.transaction
            .take()
            .ok_or_else(|| invalid("no active transaction"))?;
        Ok(())
    }

    /// Finish with an uncompressed cross-reference stream, flush, and return the
    /// destination. `xref_id` must be unused. Requires no active transaction.
    /// The cross-reference stream length must fit in `i32`, like other streams.
    pub fn finish(mut self, xref_id: Ref) -> io::Result<W> {
        if self.transaction.is_some() {
            return Err(invalid("cannot finish an active transaction"));
        }
        self.pdf.check()?;
        let start = self.pdf.len();
        self.index.insert(xref_id, start)?;
        let size = self
            .index
            .max_id
            .checked_add(1)
            .ok_or_else(|| invalid("object count exceeded"))?;
        let length = (size as u64) * 11;
        let mut reader = self.index.reader()?;
        let trailer = self.pdf.trailer_data;
        let mut chunk = self.pdf.chunk;
        let mut stream = chunk.stream_from(xref_id, &mut reader, length)?;
        stream.pair(Name(b"Type"), Name(b"XRef"));
        stream.insert(Name(b"W")).array().items([1, 8, 2]);
        trailer.write_into_dict(&mut stream, size);
        stream.finish();
        crate::finish_trailer(chunk.output.as_output(), start, b"");
        chunk.finish()
    }
}

impl<W: RollbackWrite> FilePdf<W> {
    /// Discard the current transaction, restoring bytes, object IDs, limits and
    /// trailer metadata. A successful rollback clears destination errors from
    /// the discarded suffix. The caller must first resolve transient causes
    /// such as cancellation. Index errors and failed rollbacks remain terminal.
    /// Rollback scans the scratch index using constant memory.
    pub fn rollback_transaction(&mut self) -> io::Result<()> {
        if self.index_failed {
            return self.pdf.check();
        }
        let transaction = self
            .transaction
            .take()
            .ok_or_else(|| invalid("no active transaction"))?;
        let result = self
            .index
            .rollback(transaction.count, transaction.max_id)
            .and_then(|()| self.pdf.rollback_transaction());
        if let Err(error) = result {
            self.index_failed = true;
            self.pdf.chunk.output.fail(error);
            return self.pdf.check();
        }
        Ok(())
    }
}

// Slots contain (byte offset, insertion ordinal). Ordinals let rollback remove
// out-of-order IDs without retaining an undo log in memory.
struct Index {
    file: File,
    count: u64,
    max_id: i32,
}

impl Index {
    fn insert(&mut self, id: Ref, offset: u64) -> io::Result<()> {
        if id.get() > self.max_id {
            self.file.set_len((id.get() as u64 + 1) * 16)?;
            self.max_id = id.get();
        }
        self.file.seek(SeekFrom::Start(id.get() as u64 * 16))?;
        let mut slot = [0; 16];
        self.file.read_exact(&mut slot)?;
        if slot[8..] != [0; 8] {
            return Err(invalid("duplicate indirect reference id"));
        }
        self.count += 1;
        slot[..8].copy_from_slice(&offset.to_be_bytes());
        slot[8..].copy_from_slice(&self.count.to_be_bytes());
        self.file.seek(SeekFrom::Start(id.get() as u64 * 16))?;
        self.file.write_all(&slot)
    }

    fn rollback(&mut self, count: u64, max_id: i32) -> io::Result<()> {
        let mut slot = [0; 16];
        self.file.rewind()?;
        for id in 0..=max_id {
            self.file.read_exact(&mut slot)?;
            let ordinal = u64::from_be_bytes(slot[8..].try_into().unwrap());
            if ordinal > count {
                self.file.seek(SeekFrom::Start(id as u64 * 16))?;
                self.file.write_all(&[0; 16])?;
            }
        }
        self.file.set_len((max_id as u64 + 1) * 16)?;
        self.count = count;
        self.max_id = max_id;
        Ok(())
    }

    fn reader(&mut self) -> io::Result<impl Read + '_> {
        self.file.rewind()?;
        Ok(XRefReader {
            file: BufReader::new(&mut self.file),
            entry: [0; 11],
            at: 11,
        })
    }
}

struct XRefReader<'a> {
    file: BufReader<&'a mut File>,
    entry: [u8; 11],
    at: usize,
}

impl Read for XRefReader<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        if self.at == self.entry.len() {
            let mut slot = [0; 16];
            self.file.read_exact(&mut slot)?;
            self.entry.fill(0);
            if slot[8..] == [0; 8] {
                // Permanently free entries (generation 65535) need not belong
                // to the reusable free-object chain. Object zero points to itself.
                self.entry[9..].fill(255);
            } else {
                self.entry[0] = 1;
                self.entry[1..9].copy_from_slice(&slot[..8]);
            }
            self.at = 0;
        }
        let len = bytes.len().min(self.entry.len() - self.at);
        bytes[..len].copy_from_slice(&self.entry[self.at..self.at + len]);
        self.at += len;
        Ok(len)
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
