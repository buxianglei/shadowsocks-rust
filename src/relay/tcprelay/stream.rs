// The MIT License (MIT)

// Copyright (c) 2015 Y. T. Chung

// Permission is hereby granted, free of charge, to any person obtaining a copy of
// this software and associated documentation files (the "Software"), to deal in
// the Software without restriction, including without limitation the rights to
// use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of
// the Software, and to permit persons to whom the Software is furnished to do so,
// subject to the following conditions:

// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.

// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS
// FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR
// COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER
// IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
// CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

#![allow(dead_code)]

use std::io::{IoResult, IoError, IoErrorKind};
use std::cmp;
use std::slice;

use crypto::cipher::Cipher;

pub struct DecryptedReader<R: Reader> {
    reader: R,
    buffer: Vec<u8>,
    cipher: Box<Cipher + Send>,
    pos: usize,
    sent_final: bool,
}

const BUFFER_SIZE: usize = 2048;

impl<R: Reader> DecryptedReader<R> {
    pub fn new(r: R, cipher: Box<Cipher + Send>) -> DecryptedReader<R> {
        DecryptedReader {
            reader: r,
            buffer: Vec::new(),
            cipher: cipher,
            pos: 0,
            sent_final: false,
        }
    }

    pub fn get_ref(&self) -> &R {
        &self.reader
    }

    /// Gets a mutable reference to the underlying reader.
    ///
    /// # Warning
    ///
    /// It is inadvisable to read directly from or write directly to the
    /// underlying reader.
    pub fn get_mut(&mut self) -> &mut R {
        &mut self.reader
    }

    /// Unwraps this `DecryptedReader`, returning the underlying reader.
    ///
    /// The internal buffer is flushed before returning the reader. Any leftover
    /// data in the read buffer is lost.
    pub fn into_inner(self) -> R {
        self.reader
    }
}

impl<R: Reader> Buffer for DecryptedReader<R> {
    fn fill_buf<'b>(&'b mut self) -> IoResult<&'b [u8]> {
        if self.pos == self.buffer.len() {
            let mut incoming = [0u8; BUFFER_SIZE];
            match self.reader.read(&mut incoming) {
                Ok(l) => {
                    self.buffer = match self.cipher.update(&incoming[0..l]) {
                        Ok(ret) => ret,
                        Err(err) => return Err(IoError {
                                        kind: IoErrorKind::OtherIoError,
                                        desc: err.desc,
                                        detail: err.detail,
                                    }),
                    }
                },
                Err(err) => {
                    match err.kind {
                        IoErrorKind::EndOfFile => {
                            if self.sent_final {
                                return Err(err);
                            }

                            self.sent_final = true;
                            self.buffer = match self.cipher.finalize() {
                                Ok(ret) => ret,
                                Err(err) => return Err(IoError {
                                    kind: IoErrorKind::OtherIoError,
                                    desc: err.desc,
                                    detail: err.detail,
                                }),
                            };

                            if self.buffer.len() == 0 {
                                return Err(err);
                            }
                        },
                        _ => return Err(err),
                    }
                }
            };

            self.pos = 0;
        }

        Ok(&self.buffer[self.pos..self.buffer.len()])
    }

    fn consume(&mut self, amt: usize) {
        self.pos += amt;
        assert!(self.pos <= self.buffer.len());
    }
}

impl<R: Reader> Reader for DecryptedReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        let nread = {
            let available = try!(self.fill_buf());
            let nread = cmp::min(available.len(), buf.len());
            slice::bytes::copy_memory(buf, &available[0..nread]);
            nread
        };
        self.pos += nread;
        Ok(nread)
    }
}

pub struct EncryptedWriter<W: Writer> {
    writer: W,
    cipher: Box<Cipher + Send>,
}

impl<W: Writer> EncryptedWriter<W> {
    pub fn new(w: W, cipher: Box<Cipher + Send>) -> EncryptedWriter<W> {
        EncryptedWriter {
            writer: w,
            cipher: cipher,
        }
    }

    pub fn finalize(&mut self) -> IoResult<()> {
        match self.cipher.finalize() {
            Ok(fin) => {
                self.writer.write(fin.as_slice())
            },
            Err(err) => {
                Err(IoError {
                    kind: IoErrorKind::OtherIoError,
                    desc: err.desc,
                    detail: err.detail,
                })
            }
        }
    }

    pub fn get_ref(&self) -> &W {
        &self.writer
    }

    /// Gets a mutable reference to the underlying writer.
    ///
    /// # Warning
    ///
    /// It is inadvisable to read directly from or write directly to the
    /// underlying writer.
    pub fn get_mut(&mut self) -> &mut W {
        &mut self.writer
    }
}

impl<W: Writer> Writer for EncryptedWriter<W> {
    fn write(&mut self, buf: &[u8]) -> IoResult<()> {
        match self.cipher.update(buf) {
            Ok(ret) => {
                self.writer.write(ret.as_slice())
            },
            Err(err) => {
                Err(IoError {
                    kind: IoErrorKind::OtherIoError,
                    desc: err.desc,
                    detail: err.detail,
                })
            }
        }
    }

    // fn flush(&mut self) -> IoResult<()> {
    //     self.finalize()
    // }
}

#[unsafe_destructor]
impl<W: Writer> Drop for EncryptedWriter<W> {
    fn drop(&mut self) {
        self.finalize().unwrap()
    }
}
