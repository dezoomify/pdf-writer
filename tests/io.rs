use std::cell::Cell;
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Cursor, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use pdf_writer::types::{PostScriptOp, SystemInfo, UnicodeCmap};
use pdf_writer::{
    Chunk, Content, Filter, Name, Pdf, Rect, Ref, Settings, Str, XRefFilter,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "pdf-writer-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn file(&self, name: &str) -> File {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(self.0.join(name))
            .unwrap()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

/// Deliberately implements only Write, with short writes and controllable failures.
#[derive(Debug)]
struct TestWriter<'a> {
    bytes: Vec<u8>,
    limit: &'a Cell<usize>,
    max_write: usize,
    interrupt: bool,
    fail_flush: bool,
    failures: usize,
    flushes: usize,
}

impl<'a> TestWriter<'a> {
    fn new(limit: &'a Cell<usize>) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
            max_write: 2,
            interrupt: true,
            fail_flush: false,
            failures: 0,
            flushes: 0,
        }
    }
}

impl Write for TestWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.interrupt {
            self.interrupt = false;
            return Err(io::ErrorKind::Interrupted.into());
        }
        let remaining = self.limit.get().saturating_sub(self.bytes.len());
        if remaining == 0 {
            self.failures += 1;
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "write failure"));
        }
        let len = bytes.len().min(self.max_write).min(remaining);
        self.bytes.extend_from_slice(&bytes[..len]);
        Ok(len)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flushes += 1;
        if self.fail_flush {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "flush failure"));
        }
        Ok(())
    }
}

fn document<W: Write>(pdf: &mut Pdf<W>) {
    pdf.catalog(Ref::new(1)).pages(Ref::new(2));
    pdf.pages(Ref::new(2)).kids([Ref::new(3)]).count(1);
    pdf.page(Ref::new(3))
        .parent(Ref::new(2))
        .media_box(Rect::new(0.0, 0.0, 595.0, 842.0))
        .contents(Ref::new(4))
        .resources()
        .fonts()
        .pair(Name(b"F1"), Ref::new(5));
    pdf.type1_font(Ref::new(5)).base_font(Name(b"Helvetica"));
    let mut content = Content::new(Vec::new());
    content
        .begin_text()
        .set_font(Name(b"F1"), 12.0)
        .show(Str(b"Hello (world)!"))
        .end_text();
    let limits = content.limits().clone();
    pdf.stream(Ref::new(4), &content.finish().unwrap());
    pdf.merge_limits(&limits);
}

fn finish<W: Write>(pdf: Pdf<W>, mode: u8) -> io::Result<W> {
    match mode {
        0 => pdf.finish(),
        1 => pdf.finish_with_xref_stream(Ref::new(6)),
        2 => pdf.finish_with_xref_stream_and_filter(Ref::new(6), |data| {
            let compressed = miniz_oxide::deflate::compress_to_vec_zlib(data, 6);
            (compressed, XRefFilter::Single(Filter::FlateDecode))
        }),
        _ => unreachable!(),
    }
}

fn render<W: Write>(writer: W, pretty: bool, mode: u8) -> io::Result<W> {
    let mut pdf = Pdf::with_settings(writer, Settings { pretty });
    document(&mut pdf);
    finish(pdf, mode)
}

#[test]
fn all_destinations_produce_identical_pdfs() {
    let dir = TempDir::new();
    for pretty in [false, true] {
        for mode in 0..3 {
            let expected = render(Vec::new(), pretty, mode).unwrap();
            let mut borrowed = Vec::new();
            render(&mut borrowed, pretty, mode).unwrap();
            assert_eq!(borrowed, expected);

            let limit = Cell::new(usize::MAX);
            let writer = render(TestWriter::new(&limit), pretty, mode).unwrap();
            assert_eq!(writer.bytes, expected);
            assert_eq!(writer.flushes, 1);

            for buffered in [false, true] {
                let mut file = dir.file(&format!("{pretty}-{mode}-{buffered}.pdf"));
                if buffered {
                    render(BufWriter::with_capacity(23, &mut file), pretty, mode)
                        .unwrap();
                } else {
                    render(&mut file, pretty, mode).unwrap();
                }
                file.rewind().unwrap();
                let mut actual = Vec::new();
                file.read_to_end(&mut actual).unwrap();
                assert_eq!(actual, expected);
            }
        }
    }
}

#[test]
fn object_drop_failures_are_reported_once() {
    for kind in 0..4 {
        let limit = Cell::new(usize::MAX);
        let mut writer = TestWriter::new(&limit);
        writer.fail_flush = true;
        let mut pdf = Pdf::new(&mut writer);
        match kind {
            0 => {
                let mut array = pdf.indirect(Ref::new(1)).array();
                array.item(42);
                limit.set(0);
            }
            1 => {
                let mut dict = pdf.indirect(Ref::new(1)).dict();
                dict.pair(Name(b"Key"), 42);
                limit.set(0);
            }
            2 => {
                let _stream = pdf.stream(Ref::new(1), b"payload");
                limit.set(0);
            }
            3 => {
                let mut dict = pdf.indirect(Ref::new(1)).dict();
                let mut array = dict.insert(Name(b"Key")).array();
                array.item(42);
                limit.set(0);
            }
            _ => unreachable!(),
        }
        let error = pdf.finish().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        assert_eq!(error.to_string(), "write failure");
        assert_eq!(writer.failures, 1);
        assert_eq!(writer.flushes, 0);
    }
}

#[test]
fn header_failure_precedes_finalization_and_filtering() {
    let limit = Cell::new(0);
    let mut writer = TestWriter::new(&limit);
    let mut pdf = Pdf::new(&mut writer);
    pdf.indirect(Ref::new(1)).primitive(1);
    pdf.indirect(Ref::new(1)).primitive(2);
    let error = pdf
        .finish_with_xref_stream_and_filter(Ref::new(2), |_| {
            panic!("a failed PDF must not invoke its filter")
        })
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    assert_eq!(writer.failures, 1);
}

#[test]
fn finalization_and_flush_errors_are_reported() {
    for mode in 0..3 {
        let limit = Cell::new(usize::MAX);
        let mut writer = TestWriter::new(&limit);
        let mut pdf = Pdf::new(&mut writer);
        document(&mut pdf);
        limit.set(pdf.len() as usize + 2);
        assert_eq!(finish(pdf, mode).unwrap_err().kind(), io::ErrorKind::BrokenPipe);
        assert_eq!(writer.failures, 1);

        let mut writer = TestWriter::new(&limit);
        limit.set(usize::MAX);
        writer.fail_flush = true;
        assert_eq!(
            render(&mut writer, true, mode).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(writer.flushes, 1);
    }
}

#[test]
fn zero_writes_and_partial_progress_are_handled() {
    let limit = Cell::new(usize::MAX);
    let mut writer = TestWriter::new(&limit);
    writer.max_write = 0;
    assert_eq!(
        Pdf::new(&mut writer).finish().unwrap_err().kind(),
        io::ErrorKind::WriteZero
    );

    limit.set(3);
    let mut pdf = Pdf::new(TestWriter::new(&limit));
    assert_eq!(pdf.len(), 3);
    pdf.indirect(Ref::new(1)).primitive(42);
    assert_eq!(pdf.len(), 3);
    assert_eq!(pdf.finish().unwrap_err().kind(), io::ErrorKind::BrokenPipe);
}

#[test]
fn content_operation_drop_and_flush_errors_are_reported() {
    let limit = Cell::new(usize::MAX);
    let mut writer = TestWriter::new(&limit);
    let mut content = Content::new(&mut writer);
    {
        let mut operation = content.op("custom");
        operation.operand(42);
        limit.set(0);
    }
    assert_eq!(content.finish().unwrap_err().kind(), io::ErrorKind::BrokenPipe);
    assert_eq!(writer.failures, 1);

    limit.set(usize::MAX);
    let mut writer = TestWriter::new(&limit);
    writer.fail_flush = true;
    assert_eq!(
        Content::new(&mut writer).finish().unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
}

fn objects<W: Write>(chunk: &mut Chunk<W>) {
    chunk.indirect(Ref::new(1)).dict().pair(Name(b"Self"), Ref::new(1));
    chunk.stream(Ref::new(2), b"1 0 R inside a stream");
}

#[test]
fn chunks_can_be_read_from_files_and_reused_after_renumbering() {
    let dir = TempDir::new();
    let mut file = dir.file("chunk");
    let mut source = Chunk::new(&mut file);
    objects(&mut source);
    let mut memory = Chunk::new(Cursor::new(Vec::new()));
    objects(&mut memory);

    let mut from_file = Pdf::new(Vec::new());
    from_file.extend(&mut source);
    let mut from_memory = Pdf::new(Vec::new());
    from_memory.extend(&mut memory);
    assert_eq!(from_file.finish().unwrap(), from_memory.finish().unwrap());

    let renumbered = source.renumber(Vec::new(), |id| Ref::new(id.get() + 10));
    let expected = memory.renumber(Vec::new(), |id| Ref::new(id.get() + 10));
    let bytes = renumbered.finish().unwrap();
    assert_eq!(bytes, expected.finish().unwrap());
    assert!(String::from_utf8_lossy(&bytes).contains("1 0 R inside a stream"));
    assert_eq!((&**source.get_ref()).stream_position().unwrap(), source.len());

    source.indirect(Ref::new(3)).primitive(true);
    memory.indirect(Ref::new(3)).primitive(true);
    let mut from_file = Pdf::new(Vec::new());
    from_file.extend(&mut source);
    let mut from_memory = Pdf::new(Vec::new());
    from_memory.extend(&mut memory);
    assert_eq!(from_file.finish().unwrap(), from_memory.finish().unwrap());
    source.finish().unwrap();
    memory.finish().unwrap();
}

#[test]
fn chunk_destination_errors_preserve_the_source_position() {
    for renumber in [false, true] {
        let mut source = Chunk::new(Cursor::new(Vec::new()));
        objects(&mut source);
        let limit = Cell::new(3);
        let mut target = Chunk::new(TestWriter::new(&limit));
        if renumber {
            source.renumber_into(&mut target, |id| id);
        } else {
            target.extend(&mut source);
        }
        assert_eq!(target.finish().unwrap_err().kind(), io::ErrorKind::BrokenPipe);
        assert_eq!(source.get_ref().position(), source.len());
        source.indirect(Ref::new(3)).primitive(true);
        let bytes = source.finish().unwrap().into_inner();
        assert!(bytes.ends_with(b"3 0 obj\ntrue\nendobj\n\n"));
    }
}

#[derive(Clone, Copy)]
enum Failure {
    Read,
    Seek,
    Flush,
    Write,
}

struct Readback {
    inner: Cursor<Vec<u8>>,
    failure: Failure,
}

impl Write for Readback {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if matches!(self.failure, Failure::Write) {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        self.inner.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        if matches!(self.failure, Failure::Flush) {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        Ok(())
    }
}

impl Read for Readback {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if matches!(self.failure, Failure::Read) {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        self.inner.read(bytes)
    }
}

impl Seek for Readback {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        if matches!(self.failure, Failure::Seek) {
            return Err(io::ErrorKind::Unsupported.into());
        }
        self.inner.seek(position)
    }
}

#[test]
fn chunk_readback_errors_reach_the_destination() {
    for renumber in [false, true] {
        for failure in [Failure::Read, Failure::Seek, Failure::Flush, Failure::Write] {
            let mut source =
                Chunk::new(Readback { inner: Cursor::new(Vec::new()), failure });
            objects(&mut source);
            let mut target = Chunk::new(Vec::new());
            if renumber {
                source.renumber_into(&mut target, |id| id);
            } else {
                target.extend(&mut source);
            }
            let expected = match failure {
                Failure::Read => io::ErrorKind::UnexpectedEof,
                Failure::Seek => io::ErrorKind::Unsupported,
                Failure::Flush => io::ErrorKind::PermissionDenied,
                Failure::Write => io::ErrorKind::InvalidData,
            };
            assert_eq!(target.finish().unwrap_err().kind(), expected);
            if matches!(failure, Failure::Read) {
                assert_eq!(source.get_ref().inner.position(), source.len());
            }
        }
    }
}

#[test]
fn truncated_chunks_are_rejected() {
    let dir = TempDir::new();
    let mut file = dir.file("truncated");
    let mut source = Chunk::new(&mut file);
    objects(&mut source);
    source.get_ref().set_len(0).unwrap();
    let mut target = Pdf::new(Vec::new());
    target.extend(&mut source);
    assert_eq!(target.finish().unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
}

fn cmap<W: Write>(writer: W) -> UnicodeCmap<W> {
    UnicodeCmap::new(
        writer,
        Name(b"Test"),
        SystemInfo {
            registry: Str(b"Adobe"),
            ordering: Str(b"Identity"),
            supplement: 0,
        },
    )
}

#[test]
fn character_maps_stream_ranges_and_report_errors() {
    let mut expected = cmap(Vec::new());
    let limit = Cell::new(usize::MAX);
    let mut actual = cmap(TestWriter::new(&limit));
    for glyph in 0..101u16 {
        expected.pair(glyph, 'A');
        actual.pair(glyph, 'A');
    }
    assert_eq!(actual.limits().int(), 100);
    let expected = expected.finish().unwrap();
    let actual = actual.finish().unwrap();
    assert_eq!(actual.bytes, expected);
    let text = String::from_utf8(expected).unwrap();
    assert!(text.contains("100 beginbfchar\n"));
    assert!(text.contains("1 beginbfchar\n<0064> <0041>\n"));
    assert!(text.ends_with("%%EOF"));

    let mut actual = cmap(TestWriter::new(&limit));
    actual.pair(1, 'A');
    limit.set(0);
    assert_eq!(actual.finish().unwrap_err().kind(), io::ErrorKind::BrokenPipe);

    limit.set(usize::MAX);
    let mut writer = TestWriter::new(&limit);
    writer.fail_flush = true;
    assert_eq!(
        cmap(writer).finish().unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
}

#[test]
fn postscript_encoding_keeps_limits_separate_from_bytes() {
    let limit = Cell::new(usize::MAX);
    let mut writer = TestWriter::new(&limit);
    let limits = PostScriptOp::encode(&[PostScriptOp::Integer(42)], &mut writer).unwrap();
    assert_eq!(writer.bytes, b"{42}");
    assert_eq!(limits.int(), 42);
    assert_eq!(writer.flushes, 1);

    limit.set(0);
    assert_eq!(
        PostScriptOp::encode(&[], &mut writer).unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    limit.set(usize::MAX);
    let mut writer = TestWriter::new(&limit);
    writer.fail_flush = true;
    assert_eq!(
        PostScriptOp::encode(&[], &mut writer).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
}
