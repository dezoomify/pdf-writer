/// Test that `buf` is the same as the result of concatenating the strings.
#[cfg(test)]
macro_rules! test {
    ($buf:expr, $($line:literal),* $(,)?) => {{
        let buf = $buf;
        let expected = [$(&$line[..]),*].join(&b"\n"[..]);
        if buf != expected {
            println!("=========== EXPECTED =============");
            println!("{}", String::from_utf8_lossy(&expected));
            println!("============= FOUND ==============");
            println!("{}", String::from_utf8_lossy(&buf));
            println!("=============================");
            panic!("assertion failed");
        }
    }}
}

/// Test how an object is written.
#[cfg(test)]
macro_rules! test_obj {
    (|$obj:ident| $write:expr, $($tts:tt)*) => {{
        test!(crate::tests::slice_obj(|$obj| { $write; }, crate::Settings::default()), $($tts)*)
    }}
}

/// Test how an object is written, without pretty-printing.
#[cfg(test)]
macro_rules! test_obj_no_pretty {
    (|$obj:ident| $write:expr, $($tts:tt)*) => {{
        test!(crate::tests::slice_obj(|$obj| { $write; }, crate::Settings { pretty: false }), $($tts)*)
    }}
}

/// Test how a primitive object is written.
#[cfg(test)]
macro_rules! test_primitive {
    ($value:expr, $($tts:tt)*) => {
        test_obj!(|obj| obj.primitive($value), $($tts)*);
    }
}

/// Implements the `Writer`, `Rewrite` and `Typed` traits.
macro_rules! writer {
    ($ty:ident: |$obj:ident| $($tts:tt)*) => {
        impl<'a, W: std::io::Write + ?Sized> Writer<'a, W> for $ty<'a, W> {
            #[inline]
            fn start($obj: Obj<'a, W>) -> Self {
                $($tts)*
            }
        }

        impl<'a, 'any, W: std::io::Write + ?Sized + 'a> Rewrite<'a, W> for $ty<'any, W> {
            type Output = $ty<'a, W>;
        }
    };
}

/// Implements `Deref` and `DerefMut` by delegating to a field of a struct.
macro_rules! deref {
    ($a:lifetime, $dest:ident, $from:ty => $to:ty, $field:ident) => {
        impl<$a, $dest: std::io::Write + ?Sized> std::ops::Deref for $from {
            type Target = $to;

            #[inline]
            fn deref(&self) -> &Self::Target {
                &self.$field
            }
        }

        impl<$a, $dest: std::io::Write + ?Sized> std::ops::DerefMut for $from {
            #[inline]
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.$field
            }
        }
    };
}
