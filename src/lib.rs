/*!
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

# Writers
The writers contained is this crate fall roughly into two categories.

**Core writers** enable you to write arbitrary PDF objects.

- The [`Obj`] writer allows to write most fundamental PDF objects (numbers,
  strings, arrays, dictionaries, ...). It is exposed through
  [`Chunk::indirect`] to write top-level indirect objects and through
  [`Array::push`] and [`Dict::insert`] to compose objects.
- Streams are exposed through a separate [`Chunk::stream`] method since
  they _must_ be indirect objects.

**Specialized writers** for things like a _[page]_ or an _[image]_ expose the
core writer's capabilities in a strongly typed fashion.

- A [`Page`] writer, for example, is just a thin wrapper around a [`Dict`] and
  it even derefs to a dictionary in case you need to write a field that is not
  yet exposed by the typed API.
- Similarly, the [`ImageXObject`] derefs to a [`Stream`], so that the [`filter()`]
  function can be shared by all kinds of streams. The [`Stream`] in turn derefs
  to a [`Dict`] so that you can add arbitrary fields to the stream dictionary.

When you bind a writer to a variable instead of just writing a chained builder
pattern, you may need to manually drop it before starting a new object using
[`finish()`](Finish::finish) or [`drop()`].

# Minimal example
The following example creates a PDF with a single, empty A4 page.

```
use pdf_writer::{Pdf, Rect, Ref};

# fn main() -> std::io::Result<()> {
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
# Ok(())
# }
```

For more examples, check out the [examples folder] in the repository.

# Note
This crate is rather low-level. It does not allocate or validate indirect
reference IDs for you and it does not check whether you write all required
fields for an object. Refer to the [PDF specification] to make sure you create
valid PDFs.

[page]: writers::Page
[image]: writers::ImageXObject
[`filter()`]: Stream::filter
[examples folder]: https://github.com/typst/pdf-writer/tree/main/examples
[PDF specification]: https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/PDF32000_2008.pdf
*/

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![allow(clippy::wrong_self_convention)]

#[macro_use]
mod macros;
mod actions;
mod annotations;
mod attributes;
mod chunk;
mod color;
mod content;
mod files;
mod font;
mod forms;
mod functions;
mod limits;
mod object;
mod output;
mod renditions;
mod renumber;
mod structure;
mod transitions;
mod xobject;

/// Strongly typed writers for specific PDF structures.
pub mod writers {
    use super::*;
    pub use actions::{Action, AdditionalActions, Fields};
    pub use annotations::{
        Annotation, Appearance, AppearanceCharacteristics, AppearanceEntry, BorderStyle,
        IconFit,
    };
    pub use attributes::{
        ArtifactAttributes, Attributes, FENoteAttributes, FieldAttributes,
        LayoutAttributes, ListAttributes, TableAttributes, TrackSizes, UserProperty,
    };
    pub use color::{
        ColorSpace, DeviceN, DeviceNAttrs, DeviceNMixingHints, DeviceNProcess,
        FunctionShading, IccProfile, OutputIntent, Separation, SeparationInfo,
        ShadingPattern, StreamShading, StreamShadingType, TilingPattern,
    };
    pub use content::{
        Artifact, ExtGraphicsState, MarkContent, Operation, PositionedItems,
        PropertyList, Resources, ShowPositioned, SoftMask,
    };
    pub use files::{EmbeddedFile, EmbeddingParams, FileSpec};
    pub use font::{
        CidFont, Cmap, Differences, Encoding, FontDescriptor, FontDescriptorOverride,
        Type0Font, Type1Font, Type3Font, WMode, Widths,
    };
    pub use forms::{
        CertificateSeedValue, ChoiceOptions, Field, Form, SignatureFieldLock,
        SignatureModificationDetectionPrevention, SignatureSeedValue, SignatureTimeStamp,
    };
    pub use functions::{
        ExponentialFunction, PostScriptFunction, SampledFunction, StitchingFunction,
    };
    pub use object::{
        DecodeParms, NameTree, NameTreeEntries, NumberTree, NumberTreeEntries,
    };
    pub use renditions::{MediaClip, MediaPermissions, MediaPlayParams, Rendition};
    pub use structure::{
        Catalog, ClassMap, Destination, DeveloperExtension, DocumentInfo, MarkInfo,
        MarkedRef, Metadata, Names, Namespace, NamespaceRoleMap, ObjectRef, Outline,
        OutlineItem, Page, PageLabel, Pages, RoleMap, StructChildren, StructElement,
        StructTreeRoot, ViewerPreferences,
    };
    pub use transitions::Transition;
    pub use xobject::{FormXObject, Group, ImageXObject, Reference};
}

/// Types used by specific PDF structures.
pub mod types {
    use super::*;
    pub use actions::{ActionType, FormActionFlags, RenditionOperation};
    pub use annotations::{
        AnnotationFlags, AnnotationIcon, AnnotationType, BorderType, HighlightEffect,
        IconScale, IconScaleType, TextPosition,
    };
    pub use attributes::{
        AttributeOwner, BlockAlign, FieldRole, FieldState, GlyphOrientationVertical,
        InlineAlign, LayoutBorderStyle, LayoutTextPosition, LineHeight, ListNumbering,
        NoteType, Placement, RubyAlign, RubyPosition, Sides, TableHeaderScope, TextAlign,
        TextDecorationType, WritingMode,
    };
    pub use color::{
        DeviceNSubtype, FunctionShadingType, OutputIntentSubtype, PaintType, TilingType,
    };
    pub use content::{
        ArtifactAttachment, ArtifactSubtype, ArtifactType, BlendMode, ColorSpaceOperand,
        LineCapStyle, LineJoinStyle, MaskType, OverprintMode, ProcSet, RenderingIntent,
        TextRenderingMode,
    };
    pub use files::AssociationKind;
    pub use font::{
        CidFontType, CjkClass, FontFlags, FontStretch, GlyphId, SystemInfo, UnicodeCmap,
    };
    pub use forms::{
        AccessPermissions, CertificateKeyUsage, CertificateSeedValueFlags,
        CertificateUrlType, CheckBoxState, FieldFlags, FieldType, LockingIntent,
        MdpSignatureType, Quadding, SeedValueParserVersion, SigFlags,
        SignatureDigestMethod, SignatureLockAction, SignatureSeedValueFlags,
    };
    pub use functions::{InterpolationOrder, PostScriptOp};
    pub use object::Predictor;
    pub use renditions::{MediaClipType, RenditionType, TempFileType};
    pub use structure::{
        BlockLevelRoleSubtype, Direction, InlineLevelRoleSubtype,
        InlineLevelRoleSubtype2, NumberingStyle, OutlineItemFlags, PageLayout, PageMode,
        PhoneticAlphabet, RoleMapOpts, StructRole, StructRole2, StructRole2Compat,
        StructRoleType, StructRoleType2, TabOrder, TrappingStatus,
    };
    pub use transitions::{TransitionAngle, TransitionStyle};
    pub use xobject::SMaskInData;
}

pub use self::chunk::{Chunk, Settings};
pub use self::content::Content;
pub use self::limits::Limits;
pub use self::object::{
    Array, Date, Dict, Filter, Finish, LanguageIdentifier, Name, Null, Obj, Primitive,
    Rect, Ref, Rewrite, Str, Stream, TextStr, TextStrLike, TextStrWithLang, TypedArray,
    TypedDict, Writer,
};
use self::output::{Output, Sink};

use std::fmt::{self, Debug, Formatter, Write as _};
use std::io::{self, Write};
use std::ops::{Deref, DerefMut};

use self::writers::*;

/// A builder for a PDF file.
///
/// This type writes a PDF file to a caller-supplied destination. A PDF file
/// mostly consists of indirect objects. For more
/// flexibility, you can write these objects either directly into a [`Pdf`] or
/// into a [`Chunk`], which you can add to the [`Pdf`] (or another chunk) later.
/// Therefore, most writing methods are exposed on the chunk type, which this
/// type dereferences to.
#[must_use = "call finish to write the trailer and check for I/O errors"]
pub struct Pdf<W: Write> {
    chunk: Chunk<W>,
    trailer_data: TrailerData,
}

/// Options applied before the PDF header is written.
#[derive(Debug, Clone, Copy)]
pub struct PdfOptions {
    /// Formatting settings for PDF objects.
    pub settings: Settings,
    /// PDF header version. Each component must be a single digit. Defaults to 1.7.
    pub version: (u8, u8),
    /// Four bytes identifying the file as binary. Defaults to `[0x80; 4]`.
    /// Use ASCII bytes for ASCII output.
    pub binary_marker: [u8; 4],
}

impl Default for PdfOptions {
    fn default() -> Self {
        Self {
            settings: Settings::default(),
            version: (1, 7),
            binary_marker: [0x80; 4],
        }
    }
}

impl<W: Write> Pdf<W> {
    /// Create a PDF writing to the supplied destination.
    ///
    /// The destination must be empty and positioned at its beginning. Pass a
    /// [`std::io::BufWriter`] for efficient file output. I/O errors, including
    /// errors writing the header, are reported by [`finish`](Self::finish).
    pub fn new(writer: W) -> Self {
        Self::with_options(writer, PdfOptions::default())
    }

    /// Create a PDF with the given formatting settings.
    pub fn with_settings(writer: W, settings: Settings) -> Self {
        Self::with_options(writer, PdfOptions { settings, ..PdfOptions::default() })
    }

    /// Create a PDF with the given header and formatting options.
    ///
    /// Panics if either version component exceeds 9.
    pub fn with_options(writer: W, options: PdfOptions) -> Self {
        let (major, minor) = options.version;
        assert!(major < 10 && minor < 10, "PDF version components must be single digits");
        let mut chunk = Chunk::with_settings(writer, options.settings);
        let output = chunk.output.as_output();
        output.extend(b"%PDF-");
        output.extend(&[b'0' + major, b'.', b'0' + minor]);
        output.extend(b"\n%");
        output.extend(&options.binary_marker);
        output.extend(b"\n\n");
        Self { chunk, trailer_data: TrailerData::default() }
    }

    /// Set the file identifier for the document.
    ///
    /// The file identifier is a pair of two byte strings that shall be used to
    /// uniquely identify a particular file. The first string should always stay
    /// the same for a document, the second should change for each revision. It
    /// is optional, but recommended. In PDF/A, this is required. PDF 1.1+.
    pub fn set_file_id(&mut self, id: (Vec<u8>, Vec<u8>)) {
        self.trailer_data.file_id = Some(id);
    }

    /// Start writing the document catalog. Required.
    ///
    /// This will also register the document catalog with the file trailer,
    /// meaning that you don't need to provide the given `id` anywhere else.
    pub fn catalog(&mut self, id: Ref) -> Catalog<'_> {
        self.trailer_data.catalog_id = Some(id);
        self.indirect(id).start()
    }

    /// Start writing the document information.
    ///
    /// This will also register the document information dictionary with the
    /// file trailer, meaning that you don't need to provide the given `id`
    /// anywhere else.
    pub fn document_info(&mut self, id: Ref) -> DocumentInfo<'_> {
        self.trailer_data.info_id = Some(id);
        self.indirect(id).start()
    }

    /// Write the cross-reference table and trailer, flush, and return the destination.
    ///
    /// Returns the first I/O error encountered during serialization or flushing.
    ///
    /// Panics if any indirect reference id was used twice.
    pub fn finish(self) -> io::Result<W> {
        let Chunk { output: mut sink, offsets, settings } = self.chunk;
        if sink.failed() {
            return sink.finish();
        }
        let output = sink.as_output();
        let trailer_data = self.trailer_data;
        let xref_offset = output.len();

        let mut writer = PlainXRefWriter::new(output);
        let xref_len = write_offsets(offsets, &mut writer);

        // Write the trailer dictionary.
        output.extend(b"trailer\n");
        let mut trailer = Obj::direct(output, 0, settings, false).dict();
        trailer_data.write_into_dict(&mut trailer, xref_len);
        trailer.finish();

        finish_trailer(output, xref_offset, b"\n");
        sink.finish()
    }

    /// Write the cross-reference stream and trailer, flush, and return the destination.
    ///
    /// Returns the first I/O error encountered during serialization or flushing.
    ///
    /// This method is functionally the same as [`Pdf::finish`], except that
    /// the cross-reference
    /// information is written as a cross-reference stream instead of a
    /// cross-reference table. Cross-reference streams usually allow for
    /// smaller file sizes since they can also be compressed (see
    /// [`Pdf::finish_with_xref_stream_and_filter`]),
    /// but are only available from PDF 1.5 onwards. It is also necessary
    /// to call this method instead of [`Pdf::finish`] in case object stream
    /// are used anywhere in the document since normal xref tables do not
    /// support object streams.
    ///
    /// `xref_id` will be the object identifier used for the cross-reference
    /// stream. As in other cases, the identifier needs to be unique throughout
    /// the whole document.
    ///
    /// Panics if any indirect reference id was used twice.
    pub fn finish_with_xref_stream(self, xref_id: Ref) -> io::Result<W> {
        self.finish_with_xref_stream_inner(xref_id, |buf| (buf, None))
    }

    /// Write the cross-reference stream and trailer, flush, and return the destination.
    ///
    /// Returns the first I/O error encountered during serialization or flushing.
    ///
    /// This method is equivalent to [`Pdf::finish_with_xref_stream`], except
    /// that it allows you to apply one or multiple filters to the xref stream
    /// via the `filter` closure. The input of the closure will be the raw
    /// content of the xref stream, and the output should be the filtered data
    /// as well as a single filter or a list of filters that need to be
    /// applied to unfilter the data in the correct order.
    pub fn finish_with_xref_stream_and_filter(
        self,
        xref_id: Ref,
        filter: impl FnOnce(&[u8]) -> (Vec<u8>, XRefFilter),
    ) -> io::Result<W> {
        self.finish_with_xref_stream_inner(xref_id, |buf| {
            let (xref_data, filter) = filter(&buf);
            (xref_data, Some(filter))
        })
    }

    fn finish_with_xref_stream_inner(
        self,
        xref_id: Ref,
        filter: impl FnOnce(Vec<u8>) -> (Vec<u8>, Option<XRefFilter>),
    ) -> io::Result<W> {
        let Chunk { output: mut sink, mut offsets, settings } = self.chunk;
        if sink.failed() {
            return sink.finish();
        }
        let output = sink.as_output();
        let trailer_data = self.trailer_data;

        // Include the reference of the xref stream in the offsets as well!
        let xref_offset = output.len();
        offsets.push((xref_id, xref_offset));
        let field_width = determine_field_width(xref_offset);

        let mut writer = XRefStreamWriter::new(field_width);
        let xref_len = write_offsets(offsets, &mut writer);

        let (xref_data, filter) = filter(writer.buf);

        let mut stream =
            Stream::start(Obj::indirect(output, xref_id, settings), &xref_data);

        stream.pair(Name(b"Type"), Name(b"XRef"));

        if let Some(filter) = filter {
            match filter {
                XRefFilter::Single(filter) => {
                    stream.filter(filter);
                }
                XRefFilter::Multiple(filters) => {
                    let mut arr = stream.insert(Name(b"Filter")).array();

                    for filter in filters {
                        arr.item(filter.to_name());
                    }
                }
            }
        }

        trailer_data.write_into_dict(stream.deref_mut(), xref_len);

        stream
            .insert(Name(b"W"))
            .array()
            .item(1)
            .item(field_width as i32)
            .item(2);

        stream.finish();

        finish_trailer(output, xref_offset, &[]);
        sink.finish()
    }
}

/// The filters used for the xref stream.
pub enum XRefFilter {
    /// A single filter.
    Single(Filter),
    /// An array of filters.
    Multiple(Vec<Filter>),
}

fn finish_trailer(output: &mut Output<'_>, xref_offset: u64, pad: &[u8]) {
    output.extend(pad);
    // Write startxref pointing to the xref stream
    output.extend(b"startxref\n");
    write!(output, "{}", xref_offset).unwrap();

    // Write EOF marker
    output.extend(b"\n%%EOF");
}

fn write_offsets(mut offsets: Vec<(Ref, u64)>, writer: &mut impl XRefWriter) -> i32 {
    offsets.sort();

    let xref_len = 1 + offsets.last().map_or(0, |p| p.0.get());
    writer.prologue(xref_len);

    if offsets.is_empty() {
        writer.write_free_entry(0, 65535);
    }

    let mut written = 0;
    for (i, (object_id, offset)) in offsets.iter().enumerate() {
        if written > object_id.get() {
            panic!("duplicate indirect reference id: {}", object_id.get());
        }

        // Fill in free list.
        let start = written;
        for free_id in start..object_id.get() {
            let mut next = free_id + 1;
            if next == object_id.get() {
                // Find next free id.
                for (used_id, _) in &offsets[i..] {
                    if next < used_id.get() {
                        break;
                    } else {
                        next = used_id.get() + 1;
                    }
                }
            }

            let gen = if free_id == 0 { 65535 } else { 0 };
            writer.write_free_entry((next % xref_len) as u64, gen);
            written += 1;
        }

        writer.write_occupied_entry(*offset, 0);
        written += 1;
    }

    xref_len
}

impl<W: Write> Debug for Pdf<W> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.pad("Pdf(..)")
    }
}

impl<W: Write> Deref for Pdf<W> {
    type Target = Chunk<W>;

    fn deref(&self) -> &Self::Target {
        &self.chunk
    }
}

impl<W: Write> DerefMut for Pdf<W> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.chunk
    }
}

#[derive(Default)]
struct TrailerData {
    catalog_id: Option<Ref>,
    info_id: Option<Ref>,
    file_id: Option<(Vec<u8>, Vec<u8>)>,
}

impl TrailerData {
    fn write_into_dict(&self, dict: &mut Dict, xref_len: i32) {
        dict.pair(Name(b"Size"), xref_len);

        if let Some(catalog_id) = self.catalog_id {
            dict.pair(Name(b"Root"), catalog_id);
        }

        if let Some(info_id) = self.info_id {
            dict.pair(Name(b"Info"), info_id);
        }

        if let Some(file_id) = &self.file_id {
            let mut ids = dict.insert(Name(b"ID")).array();
            ids.item(Str(&file_id.0));
            ids.item(Str(&file_id.1));
        }
    }
}

trait XRefWriter {
    fn prologue(&mut self, xref_len: i32);
    fn write_free_entry(&mut self, offset: u64, gen_number: u16);
    fn write_occupied_entry(&mut self, offset: u64, gen_number: u16);
}

struct XRefStreamWriter {
    buf: Vec<u8>,
    field_width: u32,
}

impl XRefStreamWriter {
    fn new(field_width: u32) -> Self {
        Self { buf: Vec::new(), field_width }
    }
}

impl XRefStreamWriter {
    fn write(&mut self, entry_type: u8, offset: u64, gen_number: u16) {
        let offset_bytes = offset.to_be_bytes();

        self.buf.push(entry_type);
        self.buf.extend(
            offset_bytes
                .iter()
                .skip(offset_bytes.len() - self.field_width as usize),
        );
        self.buf.extend_from_slice(&gen_number.to_be_bytes());
    }
}

impl XRefWriter for XRefStreamWriter {
    fn prologue(&mut self, _: i32) {}

    fn write_free_entry(&mut self, offset: u64, gen_number: u16) {
        self.write(0, offset, gen_number);
    }

    fn write_occupied_entry(&mut self, offset: u64, gen_number: u16) {
        self.write(1, offset, gen_number);
    }
}

struct PlainXRefWriter<'a> {
    output: &'a mut Output<'a>,
}

impl<'a> PlainXRefWriter<'a> {
    fn new(output: &'a mut Output<'a>) -> Self {
        Self { output }
    }
}

impl<'a> XRefWriter for PlainXRefWriter<'a> {
    fn prologue(&mut self, xref_len: i32) {
        self.output.extend(b"xref\n0 ");
        self.output.push_int(xref_len);
        self.output.push(b'\n');
    }

    fn write_free_entry(&mut self, offset: u64, gen_number: u16) {
        write!(self.output, "{offset:010} {gen_number:05} f\r\n").unwrap();
    }

    fn write_occupied_entry(&mut self, offset: u64, gen_number: u16) {
        write!(self.output, "{offset:010} {gen_number:05} n\r\n").unwrap();
    }
}

fn determine_field_width(offset: u64) -> u32 {
    (u64::BITS - offset.leading_zeros()).div_ceil(8)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Return the slice of bytes written during the execution of `f`.
    pub fn slice<F>(f: F, settings: Settings) -> Vec<u8>
    where
        F: FnOnce(&mut Pdf<Vec<u8>>),
    {
        let mut w = Pdf::with_settings(Vec::new(), settings);
        let start = w.len();
        f(&mut w);
        let end = w.len();
        let buf = w.finish().unwrap();
        buf[start as usize..end as usize].to_vec()
    }

    /// Return the slice of bytes written for an object.
    pub fn slice_obj<F>(f: F, settings: Settings) -> Vec<u8>
    where
        F: FnOnce(Obj<'_>),
    {
        let buf = slice(|w| f(w.indirect(Ref::new(1))), settings);
        if settings.pretty {
            buf[8..buf.len() - 9].to_vec()
        } else {
            buf[8..buf.len() - 8].to_vec()
        }
    }

    #[test]
    fn test_minimal() {
        let w = Pdf::new(Vec::new());
        test!(
            w.finish().unwrap(),
            b"%PDF-1.7\n%\x80\x80\x80\x80\n",
            b"xref\n0 1\n0000000000 65535 f\r",
            b"trailer\n<<\n  /Size 1\n>>",
            b"startxref\n16\n%%EOF",
        );
    }

    #[test]
    fn test_xref_free_list_short() {
        let mut w = Pdf::new(Vec::new());
        w.indirect(Ref::new(1)).primitive(1);
        w.indirect(Ref::new(2)).primitive(2);
        test!(
            w.finish().unwrap(),
            b"%PDF-1.7\n%\x80\x80\x80\x80\n",
            b"1 0 obj\n1\nendobj\n",
            b"2 0 obj\n2\nendobj\n",
            b"xref",
            b"0 3",
            b"0000000000 65535 f\r",
            b"0000000016 00000 n\r",
            b"0000000034 00000 n\r",
            b"trailer",
            b"<<\n  /Size 3\n>>",
            b"startxref\n52\n%%EOF",
        )
    }

    #[test]
    fn test_xref_free_list_long() {
        let mut w = Pdf::with_options(
            Vec::new(),
            PdfOptions { version: (1, 4), ..PdfOptions::default() },
        );
        w.indirect(Ref::new(1)).primitive(1);
        w.indirect(Ref::new(2)).primitive(2);
        w.indirect(Ref::new(5)).primitive(5);
        test!(
            w.finish().unwrap(),
            b"%PDF-1.4\n%\x80\x80\x80\x80\n",
            b"1 0 obj\n1\nendobj\n",
            b"2 0 obj\n2\nendobj\n",
            b"5 0 obj\n5\nendobj\n",
            b"xref",
            b"0 6",
            b"0000000003 65535 f\r",
            b"0000000016 00000 n\r",
            b"0000000034 00000 n\r",
            b"0000000004 00000 f\r",
            b"0000000000 00000 f\r",
            b"0000000052 00000 n\r",
            b"trailer",
            b"<<\n  /Size 6\n>>",
            b"startxref\n70\n%%EOF",
        )
    }

    #[test]
    #[should_panic(expected = "duplicate indirect reference id: 3")]
    fn test_xref_free_list_duplicate() {
        let mut w = Pdf::new(Vec::new());
        w.indirect(Ref::new(3)).primitive(1);
        w.indirect(Ref::new(5)).primitive(2);
        w.indirect(Ref::new(13)).primitive(1);
        w.indirect(Ref::new(3)).primitive(1);
        w.indirect(Ref::new(6)).primitive(2);
        w.finish().unwrap();
    }

    #[test]
    fn test_binary_marker() {
        let w = Pdf::with_options(
            Vec::new(),
            PdfOptions { binary_marker: *b"ABCD", ..PdfOptions::default() },
        );
        test!(
            w.finish().unwrap(),
            b"%PDF-1.7\n%ABCD\n",
            b"xref\n0 1\n0000000000 65535 f\r",
            b"trailer\n<<\n  /Size 1\n>>",
            b"startxref\n16\n%%EOF",
        );
    }

    #[test]
    fn field_width() {
        assert_eq!(determine_field_width(128), 1);
        assert_eq!(determine_field_width(255), 1);
        assert_eq!(determine_field_width(256), 2);
        assert_eq!(determine_field_width(u16::MAX as u64), 2);
        assert_eq!(determine_field_width(u16::MAX as u64 + 1), 3);
        assert_eq!(determine_field_width(u32::MAX as u64), 4);
        assert_eq!(determine_field_width(u32::MAX as u64 + 1), 5);
        assert_eq!(determine_field_width(u64::MAX), 8);
    }

    #[test]
    fn test_xref_stream() {
        let mut w = Pdf::new(Vec::new());
        w.indirect(Ref::new(1)).primitive(1);
        w.indirect(Ref::new(2)).primitive(2);
        w.indirect(Ref::new(5)).primitive(5);
        test!(
            w.finish_with_xref_stream(Ref::new(6)).unwrap(),
            b"%PDF-1.7\n%\x80\x80\x80\x80\n",
            b"1 0 obj\n1\nendobj\n",
            b"2 0 obj\n2\nendobj\n",
            b"5 0 obj\n5\nendobj\n",
            b"6 0 obj\n<<\n  /Length 28\n  /Type /XRef\n  /Size 7\n  /W [1 1 2]\n>>\nstream",
            // [0, 3, 255, 255], [1, 16, 0, 0], [1, 34, 0, 0], [0, 4, 0, 0], [0, 0, 0, 0], [1, 52, 0, 0], [1, 70, 0, 0]
            b"\x00\x03\xFF\xFF\x01\x10\x00\x00\x01\x22\x00\x00\x00\x04\x00\x00\x00\x00\x00\x00\x01\x34\x00\x00\x01\x46\x00\x00",
            b"endstream\nendobj\n",
            b"startxref\n70\n%%EOF",
        )
    }

    #[test]
    fn test_xref_stream_single_filter() {
        let mut w = Pdf::new(Vec::new());
        w.indirect(Ref::new(1)).primitive(1);
        test!(
            w.finish_with_xref_stream_and_filter(Ref::new(2), |_| (b"ABCDEFGH".to_vec(), XRefFilter::Single(Filter::FlateDecode))).unwrap(),
            b"%PDF-1.7\n%\x80\x80\x80\x80\n",
            b"1 0 obj\n1\nendobj\n",
            b"2 0 obj\n<<\n  /Length 8\n  /Type /XRef\n  /Filter /FlateDecode\n  /Size 3\n  /W [1 1 2]\n>>\nstream",
            b"ABCDEFGH",
            b"endstream\nendobj\n",
            b"startxref\n34\n%%EOF",
        )
    }

    #[test]
    fn test_xref_stream_multiple_filters() {
        let mut w = Pdf::new(Vec::new());
        w.indirect(Ref::new(1)).primitive(1);
        test!(
            w.finish_with_xref_stream_and_filter(Ref::new(2), |_| (b"ABCDEFGH".to_vec(), XRefFilter::Multiple(vec![Filter::AsciiHexDecode, Filter::FlateDecode]))).unwrap(),
            b"%PDF-1.7\n%\x80\x80\x80\x80\n",
            b"1 0 obj\n1\nendobj\n",
            b"2 0 obj\n<<\n  /Length 8\n  /Type /XRef\n  /Filter [/ASCIIHexDecode /FlateDecode]\n  /Size 3\n  /W [1 1 2]\n>>\nstream",
            b"ABCDEFGH",
            b"endstream\nendobj\n",
            b"startxref\n34\n%%EOF",
        )
    }

    #[test]
    fn test_xref_width2() {
        let mut w = Pdf::new(Vec::new());
        w.stream(Ref::new(1), &[b'0'; 256]);
        w.indirect(Ref::new(2)).primitive(1);
        test!(
            w.finish_with_xref_stream(Ref::new(3)).unwrap(),
            b"%PDF-1.7\n%\x80\x80\x80\x80\n",
            b"1 0 obj\n<<\n  /Length 256\n>>\nstream",
            b"0000000000000000000000000000000000000000000000000000000000000000000000000000000000000\
            000000000000000000000000000000000000000000000000000000000000000000000000000000000000000\
            000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
            b"endstream\nendobj\n",
            b"2 0 obj\n1\nendobj\n",
            b"3 0 obj\n<<\n  /Length 20\n  /Type /XRef\n  /Size 4\n  /W [1 2 2]\n>>\nstream",
            // [0, 0, 0, 255, 255], [1, 0, 16, 0, 0], [1, 0, 34, 0, 0], [1, 1, 32, 0, 0]
            b"\x00\x00\x00\xFF\xFF\x01\x00\x10\x00\x00\x01\x01\x46\x00\x00\x01\x01\x58\x00\x00",
            b"endstream\nendobj\n",
            b"startxref\n344\n%%EOF",
        )
    }
}
