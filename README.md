# pdf-writer
[![Crates.io](https://img.shields.io/crates/v/pdf-writer.svg)](https://crates.io/crates/pdf-writer)
[![Documentation](https://docs.rs/pdf-writer/badge.svg)](https://docs.rs/pdf-writer)

A step-by-step PDF writer.

The entry point is `Pdf<W>`, which writes to any destination implementing
`std::io::Write`: a `Vec<u8>`, a file, or a caller-defined writer. Use
`std::io::BufWriter` for efficient file output. Specialized object writers borrow
the destination, expose strongly typed builder methods, and close their objects
when dropped.

Always call `Pdf::finish()` to write the cross-reference information and trailer,
flush the destination, and check for I/O errors. Errors from object builders,
including their destructors, are retained until this call. `finish()` returns the
destination on success. Passing `&mut destination` keeps ownership with the caller.

`Content`, `Chunk`, and `UnicodeCmap` also take caller-supplied destinations.
Serialization limits are available separately through their `limits()` methods.

## Minimal example
The following example creates a PDF with a single, empty A4 page.

```rust
use pdf_writer::{Pdf, Rect, Ref};

// Define some indirect reference ids we'll use.
let catalog_id = Ref::new(1);
let page_tree_id = Ref::new(2);
let page_id = Ref::new(3);

// Write a document catalog and a page tree with one A4 page that uses no resources.
let file = std::fs::File::create("target/empty.pdf")?;
let mut pdf = Pdf::new(std::io::BufWriter::new(file));
pdf.catalog(catalog_id).pages(page_tree_id);
pdf.pages(page_tree_id).kids([page_id]).count(1);
pdf.page(page_id)
    .parent(page_tree_id)
    .media_box(Rect::new(0.0, 0.0, 595.0, 842.0))
    .resources();

// Write the cross-reference table and trailer, then flush the destination.
pdf.finish()?;
```

For in-memory output, construct `Pdf::new(Vec::new())` and retrieve the bytes with
`let bytes = pdf.finish()?`. Buffer capacity belongs to the caller:
`Pdf::new(Vec::with_capacity(8192))`.

Chunks that will be appended or renumbered need readable, seekable storage, such
as `Chunk::new(std::io::Cursor::new(Vec::new()))` or a file opened for reading and
writing. The final PDF destination only requires `Write`.

Header options are set at construction using `Pdf::with_options` and `PdfOptions`.

For more examples, check out the [examples folder] in the repository.

## Safety
This crate forbids unsafe code, but it depends on a few popular crates that use
unsafe internally.

## License
This crate is dual-licensed under the MIT and Apache 2.0 licenses.

[examples folder]: https://github.com/typst/pdf-writer/tree/main/examples
