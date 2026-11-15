use std::io::Write;

use pdf_writer::{Content, Name, Pdf, Rect, Ref, Str};

fn bench_alloc() -> Vec<u8> {
    Vec::with_capacity(16)
}

fn bench_static() -> Vec<u8> {
    let mut v = Vec::with_capacity(16);
    v.extend(b"q\n10 i\nQ");
    v
}

fn bench_chunks() -> Vec<u8> {
    let mut v = Vec::with_capacity(16);
    v.extend(b"q\n");
    v.extend(itoa::Buffer::new().format(10).as_bytes());
    v.extend(b" i\n");
    v.extend(b"Q\n");
    v.pop();
    v
}

fn bench_byte_by_byte() -> Vec<u8> {
    let mut v = Vec::with_capacity(16);
    v.push(b'q');
    v.push(b'\n');
    v.extend(itoa::Buffer::new().format(10).as_bytes());
    v.push(b' ');
    v.push(b'i');
    v.push(b'\n');
    v.push(b'Q');
    v.push(b'\n');
    v.pop();
    v
}

fn bench_std_format() -> Vec<u8> {
    let mut v = Vec::with_capacity(16);
    write!(v, "q\n{} i\nQ\n", 10).unwrap();
    v
}

fn bench_content() -> Vec<u8> {
    let mut c = Content::new(Vec::with_capacity(16));
    c.save_state();
    c.set_flatness(10);
    c.restore_state();
    c.finish().unwrap()
}

fn bench_new() -> Pdf<Vec<u8>> {
    Pdf::new(Vec::new())
}

fn bench_text() -> Vec<u8> {
    let mut content = Content::new(Vec::new());
    content
        .begin_text()
        .set_font(Name(b"F1"), 12.0)
        .next_line(50.0, 780.0);
    for line in 0..40 {
        content.next_line(0.0, -16.0);
        content.show(Str(std::hint::black_box(
            b"The quick brown fox jumps over the lazy dog (PDF) \\ 0123456789",
        )));
        content.set_char_spacing((line % 4) as f32 * 0.25);
    }
    content.end_text();
    content.finish().unwrap()
}

fn bench_full() -> Vec<u8> {
    let mut w = Pdf::new(Vec::new());
    w.catalog(Ref::new(1));
    w.page(Ref::new(2)).media_box(Rect::new(0.0, 0.0, 595.0, 842.0));
    w.stream(Ref::new(3), &b"ABCDEFG"[..]);
    w.finish().unwrap()
}

#[rustfmt::skip]
iai::main!(
    bench_alloc,
    bench_static,
    bench_chunks,
    bench_byte_by_byte,
    bench_std_format,
    bench_content,
    bench_text,
    bench_new,
    bench_full,
);
