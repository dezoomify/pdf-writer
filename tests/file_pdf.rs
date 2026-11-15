use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Cursor, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use pdf_writer::{FilePdf, Pdf, Ref, RollbackWrite};

struct Files(PathBuf);

impl Files {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "pdf-index-{}-{}",
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

impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn objects<W: Write>(pdf: &mut FilePdf<W>) {
    pdf.write(|pdf| {
        pdf.catalog(Ref::new(1)).pages(Ref::new(8));
    })
    .unwrap();
    pdf.write(|pdf| {
        pdf.pages(Ref::new(8)).kids([Ref::new(3)]).count(1);
    })
    .unwrap();
    pdf.write(|pdf| {
        pdf.page(Ref::new(3))
            .parent(Ref::new(8))
            .media_box(pdf_writer::Rect::new(0.0, 0.0, 16.0, 16.0))
            .resources();
    })
    .unwrap();
}

#[test]
fn out_of_order_objects_have_correct_64_bit_cross_references() {
    let files = Files::new();
    let mut pdf = FilePdf::new(Vec::new(), files.file("index")).unwrap();
    objects(&mut pdf);
    let bytes = pdf.finish(Ref::new(10)).unwrap();
    let marker = b"startxref\n";
    let at =
        bytes.windows(marker.len()).position(|w| w == marker).unwrap() + marker.len();
    let end = bytes[at..].iter().position(|b| *b == b'\n').unwrap() + at;
    let offset: usize = std::str::from_utf8(&bytes[at..end]).unwrap().parse().unwrap();
    assert!(bytes[offset..].starts_with(b"10 0 obj"));
    let at =
        offset + bytes[offset..].windows(7).position(|w| w == b"stream\n").unwrap() + 7;
    for id in 0..=10 {
        let entry = &bytes[at + id * 11..at + (id + 1) * 11];
        if [1, 3, 8, 10].contains(&id) {
            assert_eq!(entry[0], 1);
            let offset = u64::from_be_bytes(entry[1..9].try_into().unwrap()) as usize;
            assert!(bytes[offset..].starts_with(format!("{id} 0 obj").as_bytes()));
        } else {
            assert_eq!(entry, &[0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255]);
        }
    }
}

#[test]
fn rollback_restores_bytes_ids_and_trailer_before_retry() {
    let files = Files::new();
    let mut pdf =
        FilePdf::new(BufWriter::new(Cursor::new(Vec::new())), files.file("actual"))
            .unwrap();
    // Covers rollback before the first object as well as sparse IDs below and
    // above a previous high-water mark.
    pdf.begin_transaction().unwrap();
    pdf.write(|pdf| {
        pdf.catalog(Ref::new(1)).pages(Ref::new(8));
    })
    .unwrap();
    pdf.rollback_transaction().unwrap();
    pdf.write(|pdf| {
        pdf.indirect(Ref::new(20)).primitive(42);
    })
    .unwrap();
    pdf.begin_transaction().unwrap();
    assert!(pdf.begin_transaction().is_err());
    objects(&mut pdf);
    pdf.write(|pdf| {
        pdf.document_info(Ref::new(21))
            .title(pdf_writer::TextStr("discarded"));
    })
    .unwrap();
    pdf.rollback_transaction().unwrap();
    pdf.begin_transaction().unwrap();
    objects(&mut pdf);
    pdf.commit_transaction().unwrap();
    let actual = pdf.finish(Ref::new(22)).unwrap().into_inner().unwrap().into_inner();

    let mut expected = FilePdf::new(Vec::new(), files.file("expected")).unwrap();
    expected
        .write(|pdf| {
            pdf.indirect(Ref::new(20)).primitive(42);
        })
        .unwrap();
    objects(&mut expected);
    assert_eq!(actual, expected.finish(Ref::new(22)).unwrap());
}

struct Destination {
    bytes: Cursor<Vec<u8>>,
    remaining: usize,
    fail_rollback: bool,
}

impl Write for Destination {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        let len = bytes.len().min(self.remaining);
        self.remaining -= len;
        self.bytes.write(&bytes[..len])
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl RollbackWrite for Destination {
    fn rollback_to(&mut self, position: u64) -> io::Result<()> {
        if self.fail_rollback {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        self.bytes.rollback_to(position)
    }
}

#[test]
fn recoverable_output_failures_can_be_rolled_back() {
    let files = Files::new();
    for fail_rollback in [false, true] {
        let writer = Destination {
            bytes: Cursor::new(Vec::new()),
            remaining: usize::MAX,
            fail_rollback,
        };
        let mut pdf =
            FilePdf::new(writer, files.file(&format!("{fail_rollback}"))).unwrap();
        objects(&mut pdf);
        pdf.begin_transaction().unwrap();
        pdf.get_mut().remaining = 5;
        assert!(pdf
            .write(|pdf| {
                pdf.stream(Ref::new(20), b"partial image");
            })
            .is_err());
        assert!(pdf.commit_transaction().is_err());
        pdf.get_mut().remaining = usize::MAX;
        let rollback = pdf.rollback_transaction();
        if fail_rollback {
            assert!(rollback.is_err());
            assert!(pdf.finish(Ref::new(20)).is_err());
        } else {
            rollback.unwrap();
            let actual = pdf.finish(Ref::new(20)).unwrap().bytes.into_inner();
            let mut expected = FilePdf::new(Vec::new(), files.file("expected")).unwrap();
            objects(&mut expected);
            assert_eq!(actual, expected.finish(Ref::new(20)).unwrap());
        }
    }
}

#[test]
fn active_transactions_and_duplicate_ids_cannot_be_published() {
    let files = Files::new();
    let mut pdf = FilePdf::new(Vec::new(), files.file("active")).unwrap();
    pdf.begin_transaction().unwrap();
    assert!(pdf.finish(Ref::new(1)).is_err());
    let mut pdf = FilePdf::new(Cursor::new(Vec::new()), files.file("duplicate")).unwrap();
    objects(&mut pdf);
    pdf.begin_transaction().unwrap();
    assert!(pdf
        .write(|pdf| {
            pdf.indirect(Ref::new(1)).primitive(2);
        })
        .is_err());
    assert!(pdf.rollback_transaction().is_err());
    assert!(pdf.finish(Ref::new(10)).is_err());
}

#[test]
fn unreadable_index_is_terminal_and_buffered_files_match_memory() {
    let files = Files::new();
    let index = files.file("broken");
    let damage = index.try_clone().unwrap();
    let mut pdf = FilePdf::new(Cursor::new(Vec::new()), index).unwrap();
    pdf.write(|pdf| {
        pdf.indirect(Ref::new(20)).primitive(42);
    })
    .unwrap();
    pdf.begin_transaction().unwrap();
    // Simulate loss of scratch storage after earlier successful writes.
    damage.set_len(0).unwrap();
    assert!(pdf
        .write(|pdf| {
            pdf.indirect(Ref::new(1)).primitive(43);
        })
        .is_err());
    assert!(pdf.rollback_transaction().is_err());
    assert!(pdf.finish(Ref::new(21)).is_err());

    let mut pdf =
        FilePdf::new(BufWriter::new(files.file("output.pdf")), files.file("index"))
            .unwrap();
    objects(&mut pdf);
    pdf.finish(Ref::new(10)).unwrap();
    let mut expected = FilePdf::new(Vec::new(), files.file("expected")).unwrap();
    objects(&mut expected);
    assert_eq!(
        std::fs::read(files.0.join("output.pdf")).unwrap(),
        expected.finish(Ref::new(10)).unwrap()
    );
}

fn finish<W: Write>(pdf: Pdf<W>, stream: bool) -> io::Result<W> {
    if stream {
        pdf.finish_with_xref_stream(Ref::new(30))
    } else {
        pdf.finish()
    }
}

#[test]
fn memory_index_transactions_restore_references_limits_and_trailer() {
    for stream in [false, true] {
        let mut pdf = Pdf::new(Cursor::new(Vec::new()));
        pdf.indirect(Ref::new(20)).primitive(42);
        let limits = pdf.limits().clone();
        pdf.begin_transaction().unwrap();
        assert!(pdf.begin_transaction().is_err());
        pdf.catalog(Ref::new(1)).pages(Ref::new(2));
        pdf.document_info(Ref::new(21))
            .title(pdf_writer::TextStr("discarded"));
        pdf.indirect(Ref::new(22)).primitive(100_000);
        pdf.set_file_id((vec![1], vec![2]));
        pdf.rollback_transaction().unwrap();
        assert_eq!(pdf.limits(), &limits);
        pdf.begin_transaction().unwrap();
        pdf.indirect(Ref::new(1)).primitive(43);
        pdf.commit_transaction().unwrap();
        let actual = finish(pdf, stream).unwrap().into_inner();
        let mut expected = Pdf::new(Vec::new());
        expected.indirect(Ref::new(20)).primitive(42);
        expected.indirect(Ref::new(1)).primitive(43);
        assert_eq!(actual, finish(expected, stream).unwrap());
    }
}

#[test]
fn buffered_transaction_failure_can_be_retried_after_rollback() {
    for fail_rollback in [false, true] {
        let writer = Destination {
            bytes: Cursor::new(Vec::new()),
            remaining: usize::MAX,
            fail_rollback,
        };
        let mut pdf = Pdf::new(BufWriter::new(writer));
        pdf.indirect(Ref::new(1)).primitive(42);
        pdf.begin_transaction().unwrap();
        pdf.indirect(Ref::new(2)).primitive(43);
        pdf.get_mut().get_mut().remaining = 5;
        assert!(pdf.commit_transaction().is_err());
        pdf.get_mut().get_mut().remaining = usize::MAX;
        let rollback = pdf.rollback_transaction();
        if fail_rollback {
            assert!(rollback.is_err());
            assert!(pdf.finish().is_err());
        } else {
            rollback.unwrap();
            pdf.indirect(Ref::new(2)).primitive(44);
            let actual = pdf.finish().unwrap().into_parts().0.bytes.into_inner();
            let mut expected = Pdf::new(Vec::new());
            expected.indirect(Ref::new(1)).primitive(42);
            expected.indirect(Ref::new(2)).primitive(44);
            assert_eq!(actual, expected.finish().unwrap());
        }
    }
}

#[test]
fn every_finalization_mode_rejects_active_transactions() {
    for mode in 0..3 {
        let mut pdf = Pdf::new(Vec::new());
        pdf.begin_transaction().unwrap();
        let result = match mode {
            0 => pdf.finish(),
            1 => pdf.finish_with_xref_stream(Ref::new(1)),
            _ => pdf.finish_with_xref_stream_and_filter(Ref::new(1), |_| {
                panic!("active transaction")
            }),
        };
        assert!(result.is_err());
    }
}
