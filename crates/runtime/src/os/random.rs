//! Random bytes from the operating system (`compat_random` of `compat_win.c`): the guest's
//! `kern.arandom` and anything else that wants unpredictable bytes.

use std::io;

/// Fills `buffer` with cryptographically secure random bytes from the host.
///
/// Windows: `BCryptGenRandom` with the system's preferred generator, in pieces of 256 MiB (its
/// length is a `ULONG`). Unix: `/dev/urandom`.
pub fn random(buffer: &mut [u8]) -> io::Result<()> {
    fill(buffer, CHUNK, host_random)
}

/// `fill` hands `chunk`-byte pieces of `buffer` (the last one shorter) to `source`, in order,
/// and stops at the first error.
fn fill(
    buffer: &mut [u8],
    chunk: usize,
    mut source: impl FnMut(&mut [u8]) -> io::Result<()>,
) -> io::Result<()> {
    buffer.chunks_mut(chunk).try_for_each(&mut source)
}

#[cfg(windows)]
const CHUNK: usize = 0x1000_0000;

#[cfg(windows)]
fn host_random(piece: &mut [u8]) -> io::Result<()> {
    use windows_sys::Win32::Security::Cryptography::{
        BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
    };
    let length = u32::try_from(piece.len()).map_err(|_| io::ErrorKind::InvalidInput)?;
    // SAFETY: `piece` is `length` writable bytes; no algorithm handle with this flag.
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            piece.as_mut_ptr(),
            length,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::other(format!("BCryptGenRandom: NTSTATUS {status:#x}")))
    }
}

// One read of the whole buffer: the device has no length limit.
#[cfg(not(windows))]
const CHUNK: usize = usize::MAX;

#[cfg(unix)]
fn host_random(piece: &mut [u8]) -> io::Result<()> {
    use std::io::Read;
    std::fs::File::open("/dev/urandom")?.read_exact(piece)
}

#[cfg(not(any(unix, windows)))]
fn host_random(_: &mut [u8]) -> io::Result<()> {
    Err(io::ErrorKind::Unsupported.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_random_fills_the_buffer_with_fresh_bytes() {
        let (mut a, mut b) = ([0u8; 64], [0u8; 64]);
        random(&mut a).unwrap();
        random(&mut b).unwrap();
        // 2^-512 to fail by chance.
        assert_ne!(a, b);
        assert_ne!(a, [0; 64]);

        let mut large = vec![0u8; 1 << 20];
        random(&mut large).unwrap();
        let zeros = large.iter().filter(|&&byte| byte == 0).count();
        // About 4096 expected; far fewer or far more is not random.
        assert!((3_600..4_600).contains(&zeros), "{zeros} zero bytes in 1 MiB");
    }

    #[test]
    fn os_random_of_nothing_succeeds() {
        random(&mut []).unwrap();
    }

    /// The piece loop of `compat_random`, with a small piece size.
    #[test]
    fn os_random_fill_covers_the_buffer_in_pieces_and_stops_at_an_error() {
        let mut buffer = [0u8; 20];
        let mut lengths = Vec::new();
        let mut next = 0u8;
        fill(&mut buffer, 7, |piece| {
            lengths.push(piece.len());
            for byte in piece {
                next += 1;
                *byte = next;
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(lengths, [7, 7, 6]);
        assert!(buffer.iter().enumerate().all(|(i, &b)| b as usize == i + 1));

        let mut calls = 0;
        let error = fill(&mut buffer, 7, |_| {
            calls += 1;
            Err(io::ErrorKind::Other.into())
        })
        .unwrap_err();
        assert_eq!((error.kind(), calls), (io::ErrorKind::Other, 1));
    }
}
