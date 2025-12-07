use std::{
    fmt,
    io,
};

/// Adapter from io::Write to fmt::Write that keeps the error
///
/// Copied from [`ron`][1]. I can't believe this is not in [`std`]!
///
/// [1]: https://github.com/ron-rs/ron/blob/c2d90f6d40948d8566d3cc906565852143567a9c/src/options.rs#L295
pub struct FmtWriter<W> {
    writer: W,
    error: Result<(), io::Error>,
}

impl<W> FmtWriter<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            error: Ok(()),
        }
    }

    pub fn take_io_error(&mut self) -> Result<(), io::Error> {
        std::mem::replace(&mut self.error, Ok(()))
    }
}

impl<W> fmt::Write for FmtWriter<W>
where
    W: io::Write,
{
    fn write_str(&mut self, s: &str) -> fmt::Result {
        match self.writer.write_all(s.as_bytes()) {
            Ok(()) => Ok(()),
            Err(e) => {
                self.error = Err(e);
                Err(fmt::Error)
            }
        }
    }
}
