use std::io::{self, Read};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError};
use std::time::Duration;

const CHUNK: usize = 64 * 1024;

type Chunk = io::Result<Vec<u8>>;

/// Reads `body` on a helper thread and fails a read that waits longer than `idle` for bytes.
///
/// The helper thread ends when the source returns or errors, so the source needs its own
/// total deadline to bound a thread whose read never returns.
pub(crate) struct IdleTimeout {
    chunks: Receiver<Chunk>,
    pending: Vec<u8>,
    taken: usize,
    idle: Duration,
}

impl IdleTimeout {
    pub(crate) fn new(body: impl Read + Send + 'static, idle: Duration) -> Self {
        let (sender, chunks) = sync_channel(1);
        std::thread::spawn(move || pump(body, &sender));
        Self {
            chunks,
            pending: Vec::new(),
            taken: 0,
            idle,
        }
    }

    fn refill(&mut self) -> io::Result<()> {
        match self.chunks.recv_timeout(self.idle) {
            Ok(chunk) => chunk.map(|bytes| self.hold(bytes)),
            Err(RecvTimeoutError::Timeout) => Err(io::ErrorKind::TimedOut.into()),
            Err(RecvTimeoutError::Disconnected) => {
                self.hold(Vec::new());
                Ok(())
            }
        }
    }

    fn hold(&mut self, bytes: Vec<u8>) {
        self.pending = bytes;
        self.taken = 0;
    }
}

fn pump(mut body: impl Read, sender: &std::sync::mpsc::SyncSender<Chunk>) {
    let mut buffer = vec![0u8; CHUNK];
    loop {
        let chunk = body
            .read(&mut buffer)
            .map(|n| buffer.get(..n).unwrap_or_default().to_vec());
        let finished = !matches!(chunk, Ok(ref bytes) if !bytes.is_empty());
        if sender.send(chunk).is_err() || finished {
            return;
        }
    }
}

impl Read for IdleTimeout {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.taken == self.pending.len() {
            self.refill()?;
        }
        let ready = self.pending.get(self.taken..).unwrap_or_default();
        let n = ready.len().min(out.len());
        if let Some((from, to)) = ready.get(..n).zip(out.get_mut(..n)) {
            to.copy_from_slice(from);
        }
        self.taken += n;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    struct Gaps(Vec<(Duration, &'static [u8])>);

    impl Read for Gaps {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            let Some((wait, bytes)) = self.0.first().copied() else {
                return Ok(0);
            };
            self.0.remove(0);
            std::thread::sleep(wait);
            out[..bytes.len()].copy_from_slice(bytes);
            Ok(bytes.len())
        }
    }

    fn read_all(mut reader: impl Read) -> io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).map(|_| bytes)
    }

    #[test]
    fn a_body_larger_than_one_chunk_arrives_intact() {
        let data: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let reader = IdleTimeout::new(Cursor::new(data.clone()), Duration::from_secs(5));
        assert_eq!(read_all(reader).unwrap(), data);
    }

    #[test]
    fn gaps_shorter_than_the_idle_limit_never_add_up_to_a_failure() {
        let gap = Duration::from_millis(150);
        let source = Gaps(vec![(gap, b"ab"), (gap, b"cd"), (gap, b"ef"), (gap, b"gh")]);
        let reader = IdleTimeout::new(source, Duration::from_millis(400));
        assert_eq!(read_all(reader).unwrap(), b"abcdefgh");
    }

    #[test]
    fn a_gap_longer_than_the_idle_limit_fails_the_read() {
        let source = Gaps(vec![
            (Duration::ZERO, b"ab"),
            (Duration::from_secs(2), b"cd"),
        ]);
        let reader = IdleTimeout::new(source, Duration::from_millis(200));
        let error = read_all(reader).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn a_source_error_surfaces_after_the_bytes_before_it() {
        struct Failing(bool);
        impl Read for Failing {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                if std::mem::replace(&mut self.0, true) {
                    return Err(io::Error::other("boom"));
                }
                out[..2].copy_from_slice(b"ok");
                Ok(2)
            }
        }
        let mut reader = IdleTimeout::new(Failing(false), Duration::from_secs(5));
        let mut two = [0u8; 2];
        reader.read_exact(&mut two).unwrap();
        assert_eq!(&two, b"ok");
        assert_eq!(reader.read(&mut two).unwrap_err().to_string(), "boom");
    }
}
