use std::io::{self, Write};

use pdf_writer::types::{PostScriptOp, SystemInfo, UnicodeCmap};
use pdf_writer::{Chunk, Content, Filter, Name, Pdf, Ref, Settings, Str, XRefFilter};

/// Non-seekable destination exercising partial writes, interruption and failure.
#[derive(Debug)]
struct Destination {
    bytes: Vec<u8>,
    remaining: usize,
    interrupted: bool,
    failures: usize,
    flush_error: bool,
}

impl Destination {
    fn new(remaining: usize) -> Self {
        Self {
            bytes: vec![],
            remaining,
            interrupted: true,
            failures: 0,
            flush_error: false,
        }
    }
}

impl Write for Destination {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if std::mem::take(&mut self.interrupted) {
            return Err(io::ErrorKind::Interrupted.into());
        }
        if self.remaining == 0 {
            self.failures += 1;
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        let len = bytes.len().min(3).min(self.remaining);
        self.bytes.extend_from_slice(&bytes[..len]);
        self.remaining -= len;
        Ok(len)
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.flush_error {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        Ok(())
    }
}

fn document<W: Write>(writer: W, pretty: bool, mode: u8) -> io::Result<W> {
    let mut pdf = Pdf::with_settings(writer, Settings { pretty });
    pdf.catalog(Ref::new(1)).pages(Ref::new(2));
    pdf.pages(Ref::new(2)).kids([Ref::new(3)]).count(1);
    pdf.page(Ref::new(3))
        .parent(Ref::new(2))
        .contents(Ref::new(4))
        .resources();
    pdf.stream(Ref::new(4), b"q Q");
    pdf.indirect(Ref::new(6))
        .primitive(Str(b"plain text\nwith escapes\\and unmatched )\0"));
    match mode {
        0 => pdf.finish(),
        1 => pdf.finish_with_xref_stream(Ref::new(5)),
        _ => pdf.finish_with_xref_stream_and_filter(Ref::new(5), |data| {
            (
                miniz_oxide::deflate::compress_to_vec_zlib(data, 6),
                XRefFilter::Single(Filter::FlateDecode),
            )
        }),
    }
}

#[test]
fn destinations_and_errors_at_every_byte() {
    for pretty in [false, true] {
        for mode in 0..3 {
            let expected = document(Vec::new(), pretty, mode).unwrap();
            let actual = document(Destination::new(usize::MAX), pretty, mode).unwrap();
            assert_eq!(actual.bytes, expected);
            // Covers header, nested object Drop, stream payload, xref and trailer.
            for boundary in 0..expected.len() {
                let mut writer = Destination::new(boundary);
                writer.flush_error = true;
                assert_eq!(
                    document(&mut writer, pretty, mode).unwrap_err().kind(),
                    io::ErrorKind::BrokenPipe
                );
                assert_eq!(writer.bytes, expected[..boundary]);
                assert_eq!(writer.failures, 1);
            }
            let mut writer = Destination::new(usize::MAX);
            writer.flush_error = true;
            assert_eq!(
                document(writer, pretty, mode).unwrap_err().kind(),
                io::ErrorKind::PermissionDenied
            );
        }
    }
    // A standard fixed-size slice returns zero when full.
    assert_eq!(
        Pdf::new(&mut [0u8; 3][..]).finish().unwrap_err().kind(),
        io::ErrorKind::WriteZero
    );
}

#[test]
fn buffered_file_matches_memory() {
    let path =
        std::env::temp_dir().join(format!("pdf-writer-{}.pdf", std::process::id()));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    document(std::io::BufWriter::new(file), true, 0).unwrap();
    let actual = std::fs::read(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(actual, document(Vec::new(), true, 0).unwrap());
}

#[test]
fn generic_builders_accept_borrowed_trait_objects() {
    fn nested<W: Write + ?Sized>(obj: pdf_writer::Obj<'_, W>) {
        let mut arrays = obj.array().typed::<pdf_writer::Array<'_, W>>();
        arrays.push().item(42);
        arrays.push().item(Str(b"text"));
    }

    let mut expected = Pdf::new(Vec::new());
    nested(expected.indirect(Ref::new(1)));

    let mut bytes = Vec::new();
    let destination: &mut dyn Write = &mut bytes;
    let mut pdf = Pdf::new(destination);
    nested(pdf.indirect(Ref::new(1)));
    pdf.finish().unwrap();
    assert_eq!(bytes, expected.finish().unwrap());
}

#[test]
fn content_counts_bytes_written_before_an_error() {
    let mut content = Content::new(Destination::new(4));
    content.op("customoperator");
    assert_eq!(content.len(), 4);
    assert_eq!(content.get_ref().bytes.len(), 4);
    assert_eq!(content.finish().unwrap_err().kind(), io::ErrorKind::BrokenPipe);
}

#[test]
fn reusable_chunks_and_immediate_errors() {
    let mut bytes = Vec::new();
    let mut source = Chunk::new(&mut bytes);
    source.indirect(Ref::new(7)).dict().pair(Name(b"Self"), Ref::new(7));
    let mut expected = Pdf::new(Vec::new());
    expected.indirect(Ref::new(1)).dict().pair(Name(b"Self"), Ref::new(1));
    let renumbered = source.renumber(Vec::new(), |_| Ref::new(1)).unwrap();
    let expected_bytes = expected.finish().unwrap();
    for _ in 0..2 {
        let mut pdf = Pdf::new(Vec::new());
        pdf.extend(&renumbered).unwrap();
        assert_eq!(pdf.finish().unwrap(), expected_bytes);
    }
    let mut storage = [0u8; 2];
    let mut target = Chunk::new(&mut storage[..]);
    assert_eq!(target.extend(&source).unwrap_err().kind(), io::ErrorKind::WriteZero);
    assert_eq!(target.finish().unwrap_err().kind(), io::ErrorKind::WriteZero);
    let mut failed = Chunk::new(&mut [0u8; 0][..]);
    failed.indirect(Ref::new(1)).primitive(true);
    let mut target = Chunk::new(Vec::new());
    assert_eq!(target.extend(&failed).unwrap_err().kind(), io::ErrorKind::WriteZero);
    assert_eq!(
        failed.renumber(Vec::new(), |id| id).unwrap_err().kind(),
        io::ErrorKind::WriteZero
    );
}

#[test]
fn auxiliary_encoders_report_write_and_flush_errors() {
    for flush in [false, true] {
        let make_writer = || {
            let mut writer = Destination::new(if flush { usize::MAX } else { 1 });
            writer.flush_error = flush;
            writer
        };
        let kind = if flush {
            io::ErrorKind::PermissionDenied
        } else {
            io::ErrorKind::BrokenPipe
        };
        let mut content = Content::new(make_writer());
        content.show(Str(b"text"));
        assert_eq!(content.finish().unwrap_err().kind(), kind);
        let mut cmap = UnicodeCmap::new(
            make_writer(),
            Name(b"Test"),
            SystemInfo {
                registry: Str(b"Adobe"),
                ordering: Str(b"Identity"),
                supplement: 0,
            },
        );
        for glyph in 0..101u16 {
            cmap.pair(glyph, 'A');
        }
        assert_eq!(cmap.finish().unwrap_err().kind(), kind);
        assert_eq!(
            PostScriptOp::encode(&[PostScriptOp::Integer(42)], &mut make_writer())
                .unwrap_err()
                .kind(),
            kind
        );
    }
}
