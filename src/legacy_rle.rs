#[cfg(test)]
impl<W: Write> RleEncoder<W> {
    fn new(inner: W, compression_level: i32) -> Self {
        let min_run = match compression_level {
            0 => usize::MAX,
            1..=3 => 4,
            _ => 3,
        };
        Self {
            inner,
            min_run,
            literal: Vec::with_capacity(128),
            run_byte: None,
            run_len: 0,
        }
    }

    fn finish(mut self) -> io::Result<W> {
        self.flush_run()?;
        self.flush_literal()?;
        self.inner.flush()?;
        Ok(self.inner)
    }

    fn push_byte(&mut self, byte: u8) -> io::Result<()> {
        match self.run_byte {
            Some(run_byte) if run_byte == byte && self.run_len < 128 => {
                self.run_len += 1;
            }
            Some(_) => {
                self.flush_run()?;
                self.run_byte = Some(byte);
                self.run_len = 1;
            }
            None => {
                self.run_byte = Some(byte);
                self.run_len = 1;
            }
        }
        Ok(())
    }

    fn flush_run(&mut self) -> io::Result<()> {
        let Some(run_byte) = self.run_byte.take() else {
            return Ok(());
        };
        let run_len = self.run_len;
        self.run_len = 0;

        if run_len >= self.min_run {
            self.flush_literal()?;
            self.inner.write_all(&[(127 + run_len) as u8, run_byte])?;
        } else {
            for _ in 0..run_len {
                self.literal.push(run_byte);
                if self.literal.len() == 128 {
                    self.flush_literal()?;
                }
            }
        }
        Ok(())
    }

    fn flush_literal(&mut self) -> io::Result<()> {
        if self.literal.is_empty() {
            return Ok(());
        }
        self.inner.write_all(&[(self.literal.len() - 1) as u8])?;
        self.inner.write_all(&self.literal)?;
        self.literal.clear();
        Ok(())
    }
}

#[cfg(test)]
impl<W: Write> Write for RleEncoder<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        for byte in buf {
            self.push_byte(*byte)?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flush_run()?;
        self.flush_literal()?;
        self.inner.flush()
    }
}

impl<R: Read> RleDecoder<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            pending: VecDeque::new(),
            done: false,
        }
    }

    fn fill_pending(&mut self) -> io::Result<()> {
        if self.done || !self.pending.is_empty() {
            return Ok(());
        }

        let mut control = [0_u8; 1];
        match self.inner.read_exact(&mut control) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
                self.done = true;
                return Ok(());
            }
            Err(error) => return Err(error),
        }

        match control[0] {
            0..=127 => {
                let len = control[0] as usize + 1;
                let mut literal = vec![0_u8; len];
                self.inner.read_exact(&mut literal)?;
                self.pending.extend(literal);
            }
            128 => {}
            encoded_run => {
                let len = encoded_run as usize - 127;
                let mut byte = [0_u8; 1];
                self.inner.read_exact(&mut byte)?;
                self.pending.extend(std::iter::repeat_n(byte[0], len));
            }
        }

        Ok(())
    }
}

impl<R: Read> Read for RleDecoder<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        while self.pending.is_empty() && !self.done {
            self.fill_pending()?;
        }

        let mut written = 0;
        while written < buf.len() {
            let Some(byte) = self.pending.pop_front() else {
                break;
            };
            buf[written] = byte;
            written += 1;
        }
        Ok(written)
    }
}
