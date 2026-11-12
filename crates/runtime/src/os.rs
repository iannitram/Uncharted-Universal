//! Host OS shims (roadmap 3.15; replaces `src/compat_win.c` piece by piece, #39).
//!
//! [`Description`] is one open file description: a host file plus the position that
//! sequential reads and writes share. Every host call is positioned, so the host's own file
//! pointer never carries state; positioned calls leave the position alone, as POSIX
//! `pread` and `pwrite` do, without the save-and-restore that raced in `compat_win.c`.
//! Duplicated descriptors share one `Description` (an `Arc`), and so share its position.
//!
//! Not yet wired into the legacy C runtime: the guest file layer (`file`, roadmap 3.09)
//! routes its descriptors through this type when it moves.

use std::fs::File;
use std::io::{self, SeekFrom};
use std::sync::Mutex;

/// An open file description with its own sequential position.
///
/// Open the file without `append`: the description places every write itself.
#[derive(Debug)]
pub struct Description {
    file: File,
    position: Mutex<u64>,
}

impl Description {
    /// Wrap `file`, starting at its current host position.
    pub fn new(file: File) -> io::Result<Self> {
        use std::io::Seek;
        let position = (&file).stream_position()?;
        Ok(Self {
            file,
            position: Mutex::new(position),
        })
    }

    /// Read at the shared position and advance it by the bytes read.
    ///
    /// The position is held for the whole host call, so concurrent sequential calls on one
    /// description never hand out the same bytes twice or step over any.
    pub fn read(&self, buffer: &mut [u8]) -> io::Result<usize> {
        let mut position = self.lock();
        let n = read_at(&self.file, buffer, *position)?;
        *position += n as u64;
        Ok(n)
    }

    /// Write at the shared position and advance it by the bytes written.
    pub fn write(&self, buffer: &[u8]) -> io::Result<usize> {
        let mut position = self.lock();
        let n = write_at(&self.file, buffer, *position)?;
        *position += n as u64;
        Ok(n)
    }

    /// Read at `offset`; the shared position is unchanged (`pread`).
    pub fn read_at(&self, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
        read_at(&self.file, buffer, offset)
    }

    /// Write at `offset`; the shared position is unchanged (`pwrite`).
    pub fn write_at(&self, buffer: &[u8], offset: u64) -> io::Result<usize> {
        write_at(&self.file, buffer, offset)
    }

    /// Move the shared position (`lseek`) and return it. A target before the start of the
    /// file, or past `i64::MAX` (an `off_t`), is refused with `InvalidInput` (`EINVAL`) and
    /// the position stays where it was.
    pub fn seek(&self, to: SeekFrom) -> io::Result<u64> {
        let mut position = self.lock();
        let target = match to {
            SeekFrom::Start(offset) => Some(offset),
            SeekFrom::Current(delta) => position.checked_add_signed(delta),
            SeekFrom::End(delta) => self.file.metadata()?.len().checked_add_signed(delta),
        };
        let target = target
            .filter(|&t| i64::try_from(t).is_ok())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "seek target before the start of the file or past i64::MAX",
                )
            })?;
        *position = target;
        Ok(target)
    }

    /// The shared position.
    pub fn position(&self) -> u64 {
        *self.lock()
    }

    // A poisoned lock still holds a valid position: a panic never leaves it half-updated.
    fn lock(&self) -> std::sync::MutexGuard<'_, u64> {
        self.position.lock().unwrap_or_else(|e| e.into_inner())
    }
}

// The host calls. Both are positioned; the Windows one moves the handle's own file pointer as
// a side effect, which nothing here reads.
#[cfg(unix)]
fn read_at(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    std::os::unix::fs::FileExt::read_at(file, buffer, offset)
}

#[cfg(unix)]
fn write_at(file: &File, buffer: &[u8], offset: u64) -> io::Result<usize> {
    std::os::unix::fs::FileExt::write_at(file, buffer, offset)
}

#[cfg(windows)]
fn read_at(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    // ReadFile with an OVERLAPPED offset; end of file reads as 0 bytes, as pread does.
    std::os::windows::fs::FileExt::seek_read(file, buffer, offset)
}

#[cfg(windows)]
fn write_at(file: &File, buffer: &[u8], offset: u64) -> io::Result<usize> {
    std::os::windows::fs::FileExt::seek_write(file, buffer, offset)
}

#[cfg(not(any(unix, windows)))]
fn read_at(_: &File, _: &mut [u8], _: u64) -> io::Result<usize> {
    Err(io::ErrorKind::Unsupported.into())
}

#[cfg(not(any(unix, windows)))]
fn write_at(_: &File, _: &[u8], _: u64) -> io::Result<usize> {
    Err(io::ErrorKind::Unsupported.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, OpenOptions};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;

    const WORDS: u32 = 1 << 15; // 128 KiB of made-up data
    const BYTES: u64 = WORDS as u64 * 4;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("uu-os-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Word `i` of the file is `i` (little-endian), so every 4-byte read names its offset.
    fn counters() -> Vec<u8> {
        (0..WORDS).flat_map(u32::to_le_bytes).collect()
    }

    fn word(bytes: &[u8]) -> u32 {
        u32::from_le_bytes(bytes.try_into().unwrap())
    }

    fn open(path: &std::path::Path) -> Description {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .unwrap();
        Description::new(file).unwrap()
    }

    /// A small deterministic generator, so every run tries the same interleaving seeds.
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 33
        }
    }

    /// The #39 check: two threads share one descriptor (as after `dup`), and each mixes
    /// sequential reads with positioned reads. The sequential reads must hand out every
    /// word exactly once, in order per thread; each positioned read must see its own offset.
    #[test]
    fn os_sequential_and_positioned_reads_from_two_threads_neither_skip_nor_repeat() {
        let dir = TempDir::new("mixed-reads");
        let path = dir.0.join("made-up.bin");
        fs::write(&path, counters()).unwrap();
        let shared = Arc::new(open(&path));
        let start = Arc::new(Barrier::new(2));

        let workers: Vec<_> = (0..2u64)
            .map(|seed| {
                let shared = Arc::clone(&shared);
                let start = Arc::clone(&start);
                thread::spawn(move || {
                    let mut random = Lcg(seed + 1);
                    let mut seen = Vec::new();
                    let mut chunk = [0u8; 64];
                    start.wait();
                    loop {
                        let words = 1 + random.next() as usize % 16;
                        let n = shared.read(&mut chunk[..words * 4]).unwrap();
                        if n == 0 {
                            break;
                        }
                        assert_eq!(n % 4, 0, "a regular file read stops only at the end");
                        seen.extend(chunk[..n].chunks(4).map(word));

                        let at = random.next() % WORDS as u64;
                        let mut probe = [0u8; 4];
                        assert_eq!(shared.read_at(&mut probe, at * 4).unwrap(), 4);
                        assert_eq!(word(&probe) as u64, at, "positioned read at word {at}");
                    }
                    seen
                })
            })
            .collect();

        let mut all = Vec::new();
        for worker in workers {
            let seen = worker.join().unwrap();
            assert!(seen.windows(2).all(|w| w[0] < w[1]), "out of order");
            all.extend(seen);
        }
        all.sort_unstable();
        assert_eq!(all.len(), WORDS as usize, "a word was skipped or repeated");
        assert!(all.iter().enumerate().all(|(i, &w)| w == i as u32));
        assert_eq!(shared.position(), BYTES);
    }

    /// Sequential writes from one thread and positioned writes from another, on one
    /// descriptor: the sequential stream lands contiguous, with nothing lost or overlapped.
    #[test]
    fn os_sequential_and_positioned_writes_from_two_threads_land_where_asked() {
        let dir = TempDir::new("mixed-writes");
        let path = dir.0.join("made-up.bin");
        fs::write(&path, b"").unwrap();
        let shared = Arc::new(open(&path));
        let start = Arc::new(Barrier::new(2));
        let done = Arc::new(AtomicBool::new(false));

        let positioned = {
            let (shared, start, done) =
                (Arc::clone(&shared), Arc::clone(&start), Arc::clone(&done));
            thread::spawn(move || {
                let mut random = Lcg(7);
                start.wait();
                // Rewrite words of a second region, after the sequential one, until it ends.
                while !done.load(Ordering::Relaxed) {
                    let at = WORDS as u64 + random.next() % WORDS as u64;
                    let bytes = (at as u32).to_le_bytes();
                    assert_eq!(shared.write_at(&bytes, at * 4).unwrap(), 4);
                }
            })
        };

        start.wait();
        for i in 0..WORDS {
            assert_eq!(shared.write(&i.to_le_bytes()).unwrap(), 4);
        }
        done.store(true, Ordering::Relaxed);
        positioned.join().unwrap();

        assert_eq!(shared.position(), BYTES);
        let written = fs::read(&path).unwrap();
        assert_eq!(&written[..BYTES as usize], &counters()[..]);
        for (i, w) in written[BYTES as usize..].chunks(4).enumerate() {
            let w = word(w);
            assert!(
                w == 0 || w == WORDS + i as u32,
                "word {i} of the second region is {w}"
            );
        }
    }

    #[test]
    fn os_positioned_calls_leave_the_shared_position_alone() {
        let dir = TempDir::new("position");
        let path = dir.0.join("made-up.bin");
        fs::write(&path, counters()).unwrap();
        let file = open(&path);

        let mut buffer = [0u8; 8];
        assert_eq!(file.read(&mut buffer).unwrap(), 8);
        assert_eq!((word(&buffer[..4]), word(&buffer[4..])), (0, 1));
        assert_eq!(file.position(), 8);

        assert_eq!(file.read_at(&mut buffer[..4], 400).unwrap(), 4);
        assert_eq!(word(&buffer[..4]), 100);
        assert_eq!(file.write_at(&9_999u32.to_le_bytes(), 800).unwrap(), 4);
        assert_eq!(file.position(), 8);

        assert_eq!(file.read(&mut buffer[..4]).unwrap(), 4);
        assert_eq!(
            word(&buffer[..4]),
            2,
            "the next sequential read follows the last one"
        );
        assert_eq!(file.write(&7_777u32.to_le_bytes()).unwrap(), 4);
        assert_eq!(file.position(), 16);

        assert_eq!(file.read_at(&mut buffer[..4], 12).unwrap(), 4);
        assert_eq!(word(&buffer[..4]), 7_777);
        assert_eq!(file.read_at(&mut buffer[..4], 800).unwrap(), 4);
        assert_eq!(word(&buffer[..4]), 9_999);
    }

    #[test]
    fn os_a_second_open_has_its_own_position() {
        let dir = TempDir::new("independent");
        let path = dir.0.join("made-up.bin");
        fs::write(&path, counters()).unwrap();
        let (first, second) = (open(&path), open(&path));

        let mut buffer = [0u8; 4];
        first.seek(SeekFrom::Start(40)).unwrap();
        assert_eq!(second.read(&mut buffer).unwrap(), 4);
        assert_eq!(word(&buffer), 0);
        assert_eq!(first.read(&mut buffer).unwrap(), 4);
        assert_eq!(word(&buffer), 10);
        assert_eq!((first.position(), second.position()), (44, 4));
    }

    #[test]
    fn os_new_starts_at_the_host_position() {
        use std::io::Seek;
        let dir = TempDir::new("start");
        let path = dir.0.join("made-up.bin");
        fs::write(&path, counters()).unwrap();
        let mut file = File::open(&path).unwrap();
        file.seek(SeekFrom::Start(20)).unwrap();
        let description = Description::new(file).unwrap();
        assert_eq!(description.position(), 20);
        let mut buffer = [0u8; 4];
        assert_eq!(description.read(&mut buffer).unwrap(), 4);
        assert_eq!(word(&buffer), 5);
    }

    #[test]
    fn os_seek_follows_lseek_and_refuses_a_negative_position() {
        let dir = TempDir::new("seek");
        let path = dir.0.join("made-up.bin");
        fs::write(&path, counters()).unwrap();
        let file = open(&path);

        assert_eq!(file.seek(SeekFrom::Start(100)).unwrap(), 100);
        assert_eq!(file.seek(SeekFrom::Current(-60)).unwrap(), 40);
        assert_eq!(file.seek(SeekFrom::Current(8)).unwrap(), 48);
        assert_eq!(file.seek(SeekFrom::End(-4)).unwrap(), BYTES - 4);
        let mut buffer = [0u8; 8];
        assert_eq!(
            file.read(&mut buffer).unwrap(),
            4,
            "a read stops at the end"
        );
        assert_eq!(word(&buffer[..4]), WORDS - 1);
        assert_eq!(file.read(&mut buffer).unwrap(), 0);

        for bad in [
            SeekFrom::Current(-(BYTES as i64) - 1),
            SeekFrom::End(-(BYTES as i64) - 1),
        ] {
            let error = file.seek(bad).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
            assert_eq!(file.position(), BYTES, "a refused seek keeps the position");
        }

        // Past the end: reads find nothing, and a write there extends the file.
        assert_eq!(file.seek(SeekFrom::End(8)).unwrap(), BYTES + 8);
        assert_eq!(file.read(&mut buffer).unwrap(), 0);
        assert_eq!(file.read_at(&mut buffer, BYTES + 100).unwrap(), 0);
        assert_eq!(file.write(&1u32.to_le_bytes()).unwrap(), 4);
        assert_eq!(fs::metadata(&path).unwrap().len(), BYTES + 12);
    }
}
