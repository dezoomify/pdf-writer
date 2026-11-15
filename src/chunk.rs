use super::*;

/// Settings that should be applied while writing a PDF file.
#[derive(Debug, Clone, Copy)]
pub struct Settings {
    /// Whether to enable pretty-writing. In this case, `pdf-writer` will
    /// serialize PDFs in such a way that they are easier to read by humans by
    /// applying more padding and indentation, at the cost of larger file sizes.
    /// If disabled, `pdf-writer` will serialize objects as compactly as
    /// possible, leading to better file sizes but making it harder to inspect
    /// the file manually.
    ///
    /// _Default value_: `true`.
    pub pretty: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { pretty: true }
    }
}

/// A writer for a collection of indirect PDF objects.
///
/// The destination is supplied by the caller. Use a separate chunk when objects
/// need to be written while another writer borrows the main PDF. To append or
/// renumber a chunk, use byte storage implementing `AsRef<[u8]>`, such as
/// `Vec<u8>`. Its byte view must contain exactly the bytes written by the chunk.
/// The caller owns and configures that storage.
///
/// Always call [`finish`](Self::finish), or append the chunk to a PDF, to check
/// for I/O errors. Destinations must be empty and positioned at their beginning.
#[must_use = "finish the chunk or append it to a PDF to check for I/O errors"]
pub struct Chunk<W: Write> {
    pub(crate) output: Sink<W>,
    pub(crate) offsets: Vec<(Ref, u64)>,
    pub(crate) settings: Settings,
}

impl<W: Write> Chunk<W> {
    /// Create a chunk writing to `writer` with the default settings.
    pub fn new(writer: W) -> Self {
        Self::with_settings(writer, Settings::default())
    }

    /// Create a chunk writing to `writer` with the given settings.
    pub fn with_settings(writer: W, settings: Settings) -> Self {
        Self {
            output: Sink::new(writer),
            offsets: vec![],
            settings,
        }
    }

    /// The number of bytes successfully written so far.
    #[inline]
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> u64 {
        self.output.len()
    }

    /// Get a shared reference to the destination.
    pub fn get_ref(&self) -> &W {
        &self.output.writer
    }

    /// Get the destination to configure it without writing, seeking, or changing
    /// its contents. Those operations would invalidate recorded object offsets.
    pub fn get_mut(&mut self) -> &mut W {
        &mut self.output.writer
    }

    /// Report a retained serialization error without consuming the writer.
    /// Does not flush buffered bytes; use [`flush`](Self::flush) for that.
    /// The original error remains available from [`finish`](Self::finish).
    pub fn check(&self) -> io::Result<()> {
        self.output.check()
    }

    /// Flush the destination and report serialization or flush errors without
    /// finalizing the PDF. A flush failure is retained for subsequent calls,
    /// including [`finish`](Self::finish).
    pub fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }

    /// Check for serialization errors, flush, and return the destination.
    pub fn finish(self) -> io::Result<W> {
        self.output.finish()
    }

    /// Append objects from a chunk backed by readable byte storage.
    ///
    /// Returns source or destination errors immediately. A destination failure
    /// is also retained for [`finish`](Self::finish).
    pub fn extend<R: Write + AsRef<[u8]>>(&mut self, other: &Chunk<R>) -> io::Result<()> {
        other.output.check()?;
        self.output.check()?;
        let base = self.len();
        self.output.extend(other.get_ref().as_ref());
        self.output.check()?;
        self.merge_limits(other.limits());
        self.offsets
            .extend(other.offsets.iter().map(|&(id, offset)| (id, base + offset)));
        Ok(())
    }

    /// Iterate over the top-level object references, in serialization order.
    pub fn refs(&self) -> impl ExactSizeIterator<Item = Ref> + '_ {
        self.offsets.iter().map(|&(id, _)| id)
    }

    /// Get the limits of data written into the chunk.
    pub fn limits(&self) -> &Limits {
        self.output.limits()
    }

    /// Merge limits from separately generated content into this chunk.
    ///
    /// ```
    /// use pdf_writer::{Chunk, Content, Ref};
    /// let mut content = Content::new(Vec::new());
    /// content.set_dash_pattern([1.0, 3.0, 2.0, 4.0, 5.0], 1.0);
    /// let limits = content.limits().clone();
    /// let bytes = content.finish()?;
    /// let mut chunk = Chunk::new(Vec::new());
    /// chunk.stream(Ref::new(1), &bytes);
    /// chunk.merge_limits(&limits);
    /// assert_eq!(chunk.limits().array_len(), 5);
    /// # chunk.finish()?;
    /// # Ok::<_, std::io::Error>(())
    /// ```
    pub fn merge_limits(&mut self, other: &Limits) {
        self.output.limits_mut().merge(other);
    }
}

impl<R: Write + AsRef<[u8]>> Chunk<R> {
    /// Renumber objects and references into a new caller-supplied destination.
    ///
    /// `mapping` is called for every occurrence of an ID, including references
    /// to objects outside this chunk. It must return a consistent mapping.
    /// Strings and stream payloads are copied without interpreting their bytes.
    /// Returns source or destination errors immediately.
    ///
    /// ```
    /// use std::collections::HashMap;
    /// use pdf_writer::{Chunk, Ref};
    /// let mut chunk = Chunk::new(Vec::new());
    /// chunk.indirect(Ref::new(10)).primitive(true);
    /// let mut alloc = Ref::new(1);
    /// let mut map = HashMap::new();
    /// let renumbered = chunk.renumber(Vec::new(), |old| {
    ///     *map.entry(old).or_insert_with(|| alloc.bump())
    /// })?;
    /// let bytes = renumbered.finish()?;
    /// # Ok::<_, std::io::Error>(())
    /// ```
    pub fn renumber<W: Write>(
        &self,
        writer: W,
        mapping: impl FnMut(Ref) -> Ref,
    ) -> io::Result<Chunk<W>> {
        let mut chunk = Chunk::with_settings(writer, self.settings);
        self.renumber_into(&mut chunk, mapping)?;
        Ok(chunk)
    }

    /// Renumber into an existing target chunk.
    ///
    /// Returns source or destination errors immediately.
    pub fn renumber_into<W: Write>(
        &self,
        target: &mut Chunk<W>,
        mut mapping: impl FnMut(Ref) -> Ref,
    ) -> io::Result<()> {
        self.output.check()?;
        target.output.check()?;
        crate::renumber::renumber(self, target, &mut mapping);
        target.output.check()
    }
}

/// Indirect objects and streams.
impl<W: Write> Chunk<W> {
    /// Start writing an indirectly referenceable object.
    pub fn indirect(&mut self, id: Ref) -> Obj<'_> {
        self.offsets.push((id, self.output.len()));
        Obj::indirect(self.output.as_output(), id, self.settings)
    }

    /// Start writing an indirectly referenceable stream.
    ///
    /// The stream data and the `/Length` field are written automatically. You
    /// can add additional key-value pairs to the stream dictionary with the
    /// returned stream writer.
    ///
    /// You can use this function together with a [`Content`] stream builder to
    /// provide a [page's contents](Page::contents).
    /// ```
    /// use pdf_writer::{Pdf, Content, Ref};
    ///
    /// // Create a simple content stream.
    /// let mut content = Content::new(Vec::new());
    /// content.rect(50.0, 50.0, 50.0, 50.0);
    /// content.stroke();
    ///
    /// // Create a writer and write the stream.
    /// let mut pdf = Pdf::new(Vec::new());
    /// pdf.stream(Ref::new(1), &content.finish().unwrap());
    /// ```
    ///
    /// This crate does not do any compression for you. If you want to compress
    /// a stream, you have to pass already compressed data into this function
    /// and specify the appropriate filter in the stream dictionary.
    ///
    /// For example, if you want to compress your content stream with DEFLATE,
    /// you could do something like this:
    /// ```
    /// use pdf_writer::{Pdf, Content, Ref, Filter};
    /// use miniz_oxide::deflate::{compress_to_vec_zlib, CompressionLevel};
    ///
    /// // Create a simple content stream.
    /// let mut content = Content::new(Vec::new());
    /// content.rect(50.0, 50.0, 50.0, 50.0);
    /// content.stroke();
    ///
    /// // Compress the stream.
    /// let level = CompressionLevel::DefaultLevel as u8;
    /// let compressed = compress_to_vec_zlib(&content.finish().unwrap(), level);
    ///
    /// // Create a writer, write the compressed stream and specify that it
    /// // needs to be decoded with a FLATE filter.
    /// let mut pdf = Pdf::new(Vec::new());
    /// pdf.stream(Ref::new(1), &compressed).filter(Filter::FlateDecode);
    /// ```
    /// For all the specialized stream functions below, it works the same way:
    /// You can pass compressed data and specify a filter.
    ///
    /// Panics if the stream length exceeds `i32::MAX`.
    pub fn stream<'a>(&'a mut self, id: Ref, data: &'a [u8]) -> Stream<'a> {
        Stream::start(self.indirect(id), data)
    }
}

/// Document structure.
impl<W: Write> Chunk<W> {
    /// Start writing a page tree.
    pub fn pages(&mut self, id: Ref) -> Pages<'_> {
        self.indirect(id).start()
    }

    /// Start writing a page.
    pub fn page(&mut self, id: Ref) -> Page<'_> {
        self.indirect(id).start()
    }

    /// Start writing an outline.
    pub fn outline(&mut self, id: Ref) -> Outline<'_> {
        self.indirect(id).start()
    }

    /// Start writing an outline item.
    pub fn outline_item(&mut self, id: Ref) -> OutlineItem<'_> {
        self.indirect(id).start()
    }

    /// Start writing a destination for use in a name tree.
    pub fn destination(&mut self, id: Ref) -> Destination<'_> {
        self.indirect(id).start()
    }

    /// Start writing a named destination dictionary.
    pub fn destinations(&mut self, id: Ref) -> TypedDict<'_, Destination<'_>> {
        self.indirect(id).dict().typed()
    }

    /// Start writing a file specification dictionary.
    pub fn file_spec(&mut self, id: Ref) -> FileSpec<'_> {
        self.indirect(id).start()
    }

    /// Start writing an embedded file stream.
    pub fn embedded_file<'a>(&'a mut self, id: Ref, bytes: &'a [u8]) -> EmbeddedFile<'a> {
        EmbeddedFile::start(self.stream(id, bytes))
    }

    /// Start writing a structure tree element.
    pub fn struct_element(&mut self, id: Ref) -> StructElement<'_> {
        self.indirect(id).start()
    }

    /// Start writing a namespace dictionary. PDF 2.0+
    pub fn namespace(&mut self, id: Ref) -> Namespace<'_> {
        self.indirect(id).start()
    }

    /// Start writing a metadata stream.
    pub fn metadata<'a>(&'a mut self, id: Ref, bytes: &'a [u8]) -> Metadata<'a> {
        Metadata::start(self.stream(id, bytes))
    }
}

/// Graphics and content.
impl<W: Write> Chunk<W> {
    /// Start writing an image XObject stream.
    ///
    /// The samples should be encoded according to the stream's filter, color
    /// space and bits per component.
    pub fn image_xobject<'a>(
        &'a mut self,
        id: Ref,
        samples: &'a [u8],
    ) -> ImageXObject<'a> {
        ImageXObject::start(self.stream(id, samples))
    }

    /// Start writing a form XObject stream.
    ///
    /// These can be used as transparency groups.
    ///
    /// Note that these have nothing to do with forms that have fields to fill
    /// out. Rather, they are a way to encapsulate and reuse content across the
    /// file.
    ///
    /// You can create the content bytes using a [`Content`] builder.
    pub fn form_xobject<'a>(&'a mut self, id: Ref, content: &'a [u8]) -> FormXObject<'a> {
        FormXObject::start(self.stream(id, content))
    }

    /// Start writing an external graphics state dictionary.
    pub fn ext_graphics(&mut self, id: Ref) -> ExtGraphicsState<'_> {
        self.indirect(id).start()
    }
}

/// Fonts.
impl<W: Write> Chunk<W> {
    /// Start writing a Type-1 font.
    pub fn type1_font(&mut self, id: Ref) -> Type1Font<'_> {
        self.indirect(id).start()
    }

    /// Start writing a Type-3 font.
    pub fn type3_font(&mut self, id: Ref) -> Type3Font<'_> {
        self.indirect(id).start()
    }

    /// Start writing a Type-0 font.
    pub fn type0_font(&mut self, id: Ref) -> Type0Font<'_> {
        self.indirect(id).start()
    }

    /// Start writing a CID font.
    pub fn cid_font(&mut self, id: Ref) -> CidFont<'_> {
        self.indirect(id).start()
    }

    /// Start writing a font descriptor.
    pub fn font_descriptor(&mut self, id: Ref) -> FontDescriptor<'_> {
        self.indirect(id).start()
    }

    /// Start writing a character map stream.
    ///
    /// If you want to use this for a `/ToUnicode` CMap, you can create the
    /// bytes using a [`UnicodeCmap`](types::UnicodeCmap) builder.
    pub fn cmap<'a>(&'a mut self, id: Ref, cmap: &'a [u8]) -> Cmap<'a> {
        Cmap::start(self.stream(id, cmap))
    }
}

/// Color spaces, shadings and patterns.
impl<W: Write> Chunk<W> {
    /// Start writing a color space.
    pub fn color_space(&mut self, id: Ref) -> ColorSpace<'_> {
        self.indirect(id).start()
    }

    /// Start writing a function-based shading (type 1-3).
    pub fn function_shading(&mut self, id: Ref) -> FunctionShading<'_> {
        self.indirect(id).start()
    }

    /// Start writing a stream-based shading (type 4-7).
    pub fn stream_shading<'a>(
        &'a mut self,
        id: Ref,
        content: &'a [u8],
    ) -> StreamShading<'a> {
        StreamShading::start(self.stream(id, content))
    }

    /// Start writing a tiling pattern stream.
    ///
    /// You can create the content bytes using a [`Content`] builder.
    pub fn tiling_pattern<'a>(
        &'a mut self,
        id: Ref,
        content: &'a [u8],
    ) -> TilingPattern<'a> {
        TilingPattern::start_with_stream(self.stream(id, content))
    }

    /// Start writing a shading pattern.
    pub fn shading_pattern(&mut self, id: Ref) -> ShadingPattern<'_> {
        self.indirect(id).start()
    }

    /// Start writing an ICC profile stream.
    ///
    /// The `profile` argument shall contain the ICC profile data conforming to
    /// ICC.1:2004-10 (PDF 1.7), ICC.1:2003-09 (PDF 1.6), ICC.1:2001-12 (PDF 1.5),
    /// ICC.1:1999-04 (PDF 1.4), or ICC 3.3 (PDF 1.3). Profile data is commonly
    /// compressed using the `FlateDecode` filter.
    pub fn icc_profile<'a>(&'a mut self, id: Ref, profile: &'a [u8]) -> IccProfile<'a> {
        IccProfile::start(self.stream(id, profile))
    }
}

/// Functions.
impl<W: Write> Chunk<W> {
    /// Start writing a sampled function stream.
    pub fn sampled_function<'a>(
        &'a mut self,
        id: Ref,
        samples: &'a [u8],
    ) -> SampledFunction<'a> {
        SampledFunction::start(self.stream(id, samples))
    }

    /// Start writing an exponential function.
    pub fn exponential_function(&mut self, id: Ref) -> ExponentialFunction<'_> {
        self.indirect(id).start()
    }

    /// Start writing a stitching function.
    pub fn stitching_function(&mut self, id: Ref) -> StitchingFunction<'_> {
        self.indirect(id).start()
    }

    /// Start writing a PostScript function stream.
    ///
    /// You can create the code bytes using [`PostScriptOp::encode`](types::PostScriptOp::encode).
    pub fn post_script_function<'a>(
        &'a mut self,
        id: Ref,
        code: &'a [u8],
    ) -> PostScriptFunction<'a> {
        PostScriptFunction::start(self.stream(id, code))
    }
}

/// Tree data structures.
impl<W: Write> Chunk<W> {
    /// Start writing a name tree node.
    pub fn name_tree<T: Primitive>(&mut self, id: Ref) -> NameTree<'_, T> {
        self.indirect(id).start()
    }

    /// Start writing a number tree node.
    pub fn number_tree<T: Primitive>(&mut self, id: Ref) -> NumberTree<'_, T> {
        self.indirect(id).start()
    }
}

/// Interactive features.
impl<W: Write> Chunk<W> {
    /// Start writing an annotation dictionary.
    pub fn annotation(&mut self, id: Ref) -> Annotation<'_> {
        self.indirect(id).start()
    }

    /// Start writing a form field dictionary.
    pub fn form_field(&mut self, id: Ref) -> Field<'_> {
        self.indirect(id).start()
    }

    /// Start writing a signature field lock dictionary.
    pub fn signature_field_lock(&mut self, id: Ref) -> SignatureFieldLock<'_> {
        self.indirect(id).start()
    }

    /// Start writing a seed value dictionary.
    pub fn signature_seed_value(&mut self, id: Ref) -> SignatureSeedValue<'_> {
        self.indirect(id).start()
    }
}

impl<W: Write> Debug for Chunk<W> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.pad("Chunk(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk() {
        let mut w = Pdf::new(Vec::new());
        let mut font = w.type3_font(Ref::new(1));
        let mut c = Chunk::new(Vec::new());
        c.font_descriptor(Ref::new(2)).name(Name(b"MyFont"));
        font.font_descriptor(Ref::new(2));
        font.finish();
        w.extend(&c).unwrap();
        test!(
            w.finish().unwrap(),
            b"%PDF-1.7\n%\x80\x80\x80\x80\n",
            b"1 0 obj",
            b"<<\n  /Type /Font\n  /Subtype /Type3\n  /FontDescriptor 2 0 R\n>>",
            b"endobj\n",
            b"2 0 obj",
            b"<<\n  /Type /FontDescriptor\n  /FontName /MyFont\n>>",
            b"endobj\n",
            b"xref",
            b"0 3",
            b"0000000000 65535 f\r",
            b"0000000016 00000 n\r",
            b"0000000094 00000 n\r",
            b"trailer",
            b"<<\n  /Size 3\n>>",
            b"startxref\n160\n%%EOF",
        );
    }
}
