//! Wall-clock escaping benchmark: cargo bench --bench escaping -- vec sparse 20
//! Modes: vec, file, file64. Cases: short, plain, sparse, dense, binary, long, document.
use std::hint::black_box;
use std::io::{BufWriter, Write};
use std::time::Instant;

use pdf_writer::{Content, Name, Pdf, Rect, Ref, Str};

fn serialize<W: Write>(writer: W, case: &str, text: &[u8], count: usize) -> W {
    if case == "document" {
        return document(writer);
    }
    let mut content = Content::new(writer);
    for _ in 0..count {
        content.show(Str(black_box(text)));
    }
    content.finish().unwrap()
}

fn document<W: Write>(writer: W) -> W {
    let mut pdf = Pdf::new(writer);
    pdf.catalog(Ref::new(1)).pages(Ref::new(2));
    pdf.pages(Ref::new(2))
        .count(2000)
        .kids((0..2000).map(|i| Ref::new(4 + i * 2)));
    pdf.type1_font(Ref::new(3)).base_font(Name(b"Helvetica"));
    for i in 0..2000 {
        let mut page = pdf.page(Ref::new(4 + i * 2));
        page.parent(Ref::new(2))
            .media_box(Rect::new(0.0, 0.0, 595.0, 842.0))
            .contents(Ref::new(5 + i * 2));
        page.resources().fonts().pair(Name(b"F1"), Ref::new(3));
        drop(page);
        let mut content = Content::new(Vec::new());
        content
            .begin_text()
            .set_font(Name(b"F1"), 12.0)
            .next_line(50.0, 780.0);
        for line in 0..40 {
            content.next_line(0.0, -16.0);
            content.show(Str(black_box(
                b"The quick brown fox jumps over the lazy dog (PDF) \\ 0123456789",
            )));
            content.set_char_spacing((line % 4) as f32 * 0.25);
        }
        content.end_text();
        pdf.stream(Ref::new(5 + i * 2), &content.finish().unwrap());
    }
    pdf.finish().unwrap()
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let mode = args.get(1).map_or("vec", String::as_str);
    let case = args.get(2).map_or("sparse", String::as_str);
    let repetitions: u32 = args.get(3).map_or(20, |arg| arg.parse().unwrap());
    assert!(repetitions > 0);
    let text = match case {
        "short" => b"Hello".to_vec(),
        "plain" => b"The quick brown fox jumps over the lazy dog 0123456789".to_vec(),
        "sparse" => {
            b"The quick brown fox jumps over the lazy dog (PDF) \\ 0123456789".to_vec()
        }
        "dense" => b"\0\n\r\t\x08\x0c\\)(\x7f".repeat(6),
        "binary" => (0..=255).collect(),
        "long" => b"a".repeat(16 * 1024),
        "document" => Vec::new(),
        _ => panic!("unknown case: {case}"),
    };
    let count = (8 * 1024 * 1024 / text.len().max(1)).min(100_000);
    let path = std::env::temp_dir()
        .join(format!("pdf-writer-escaping-{}.pdf", std::process::id()));
    let start = Instant::now();
    for _ in 0..repetitions {
        match mode {
            "vec" => {
                black_box(serialize(Vec::new(), case, &text, count));
            }
            "file" | "file64" => {
                let file = std::fs::File::create(&path).unwrap();
                let capacity = if mode == "file" { 8192 } else { 65536 };
                serialize(BufWriter::with_capacity(capacity, file), case, &text, count);
            }
            _ => panic!("unknown mode: {mode}"),
        }
    }
    let elapsed = start.elapsed() / repetitions;
    if mode != "vec" {
        std::fs::remove_file(path).unwrap();
    }
    println!("{mode},{case},{}", elapsed.as_nanos());
}
