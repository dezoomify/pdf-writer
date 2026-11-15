/// Tracks the limits of serialized PDF data types.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Limits {
    int: i32,
    real: f32,
    name_len: usize,
    str_len: usize,
    array_len: usize,
    dict_entries: usize,
}

impl Limits {
    /// Create a new `Limits` struct with all values initialized to zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the absolute value of the largest positive/negative integer number.
    pub fn int(&self) -> i32 {
        self.int
    }

    /// Get the absolute value of the largest positive/negative real number.
    pub fn real(&self) -> f32 {
        self.real
    }

    /// Get the maximum length of any used name.
    pub fn name_len(&self) -> usize {
        self.name_len
    }

    /// Get the maximum length of any used string.
    pub fn str_len(&self) -> usize {
        self.str_len
    }

    /// Get the maximum length of any used array.
    pub fn array_len(&self) -> usize {
        self.array_len
    }

    /// Get the maximum number of entries in any dictionary.
    pub fn dict_entries(&self) -> usize {
        self.dict_entries
    }

    pub(crate) fn register_int(&mut self, val: i32) {
        self.int = self.int.max(val.abs());
    }

    pub(crate) fn register_real(&mut self, val: f32) {
        self.real = self.real.max(val.abs());
    }

    pub(crate) fn register_name_len(&mut self, len: usize) {
        self.name_len = self.name_len.max(len);
    }

    pub(crate) fn register_str_len(&mut self, len: usize) {
        self.str_len = self.str_len.max(len);
    }

    pub(crate) fn register_array_len(&mut self, len: usize) {
        self.array_len = self.array_len.max(len);
    }

    pub(crate) fn register_dict_entries(&mut self, len: usize) {
        self.dict_entries = self.dict_entries.max(len);
    }

    /// Merge two `Limits` with each other, taking the maximum
    /// of each field from both.
    pub fn merge(&mut self, other: &Limits) {
        self.register_int(other.int);
        self.register_real(other.real);
        self.register_name_len(other.name_len);
        self.register_str_len(other.str_len);
        self.register_array_len(other.array_len);
        self.register_dict_entries(other.dict_entries);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Chunk, Content, Finish, Name, Rect, Ref, Str, TextStr};

    #[test]
    fn test_content_limits() {
        let mut limits = Limits::default();

        let mut content = Content::new(Vec::new());
        content.cubic_to(14.3, 16.2, 22.6, 30.9, 50.1, 40.0);
        content.show(Str(b"Some text"));
        content.set_font(Name(b"NotoSans"), 10.0);
        limits.merge(content.limits());
        content.finish().unwrap();

        let mut content = Content::new(Vec::new());
        content.line_to(55.0, -75.3);
        content.set_font(Name(b"Noto"), 10.0);
        content
            .show_positioned()
            .items()
            .show(Str(b"A"))
            .show(Str(b"B"))
            .adjust(32.0);
        content
            .marked_content_point_with_properties(Name(b"Hi"))
            .properties()
            .actual_text(TextStr("text"));
        limits.merge(content.limits());
        content.finish().unwrap();

        assert_eq!(
            limits,
            Limits {
                int: 55,
                real: 75.3,
                name_len: 10,
                str_len: 9,
                array_len: 3,
                dict_entries: 1,
            }
        )
    }

    #[test]
    fn test_chunk_limits() {
        let mut limits = Limits::default();

        let mut chunk = Chunk::new(Vec::new());
        let mut x_object = chunk.form_xobject(Ref::new(1), &[]);
        x_object.bbox(Rect::new(4.0, 6.0, 22.1, 31.0));
        x_object.finish();

        limits.merge(chunk.limits());

        assert_eq!(
            limits,
            Limits {
                int: 31,
                real: 22.1,
                name_len: 7,
                str_len: 0,
                array_len: 4,
                dict_entries: 4,
            }
        )
    }
}
