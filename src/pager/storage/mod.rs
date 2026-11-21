//! Raw page storage. Each backend only knows how to move whole pages in and
//! out; caching and dirty tracking live in `Pager`, written once for all of them.

mod file;
mod memory;

pub use file::FileStorage;
pub use memory::MemoryStorage;

use std::io;

pub const PAGE_SIZE: usize = 4096;
pub type Page = [u8; PAGE_SIZE];

pub trait Storage {
    /// Fill `buf` with page `n`. Errors if `n >= self.num_pages()`.
    fn read_page(&mut self, n: usize, buf: &mut Page) -> io::Result<()>;

    /// Write `buf` as page `n`, growing the storage if `n` is past the end.
    fn write_page(&mut self, n: usize, buf: &Page) -> io::Result<()>;

    /// Number of pages currently stored.
    fn num_pages(&self) -> usize;

    /// Make sure everything written so far is durable.
    fn sync(&mut self) -> io::Result<()>;
}

/// Shared error for reading a page that doesn't exist.
fn out_of_range(n: usize, num_pages: usize) -> io::Error {
    io::Error::new(
        io::ErrorKind::UnexpectedEof,
        format!("page {n} out of range ({num_pages} pages)"),
    )
}

#[cfg(test)]
pub(crate) mod test_util {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{PAGE_SIZE, Page};

    /// A unique temp file path, deleted when dropped.
    pub struct TempPath(PathBuf);

    impl TempPath {
        pub fn new() -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let id = COUNTER.fetch_add(1, Ordering::Relaxed);
            let name = format!("sqlite_clone_test_{}_{id}.db", std::process::id());
            let path = std::env::temp_dir().join(name);
            let _ = std::fs::remove_file(&path);
            Self(path)
        }
    }

    impl AsRef<Path> for TempPath {
        fn as_ref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempPath {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    /// A page where every byte is `byte`.
    pub fn filled(byte: u8) -> Box<Page> {
        Box::new([byte; PAGE_SIZE])
    }

    /// A page whose bytes form a pattern unique to `seed`, so mixed-up pages
    /// or offsets show up in tests.
    pub fn patterned(seed: u8) -> Box<Page> {
        let mut page = Box::new([0; PAGE_SIZE]);
        for (i, b) in page.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(31).wrapping_add(seed);
        }
        page
    }
}

/// The contract every backend must follow. Each test runs against every backend.
#[cfg(test)]
mod contract_tests {
    use super::test_util::{TempPath, filled, patterned};
    use super::*;

    /// Run `check` against a fresh, empty instance of every backend.
    fn for_each_backend(check: impl Fn(&mut dyn Storage)) {
        check(&mut MemoryStorage::new());
        let path = TempPath::new();
        check(&mut FileStorage::open(&path).unwrap());
    }

    fn read(storage: &mut dyn Storage, n: usize) -> Box<Page> {
        let mut buf = Box::new([0; PAGE_SIZE]);
        storage.read_page(n, &mut buf).unwrap();
        buf
    }

    #[test]
    fn starts_empty() {
        for_each_backend(|s| assert_eq!(s.num_pages(), 0));
    }

    #[test]
    fn reading_empty_storage_errors() {
        for_each_backend(|s| {
            let mut buf = Box::new([0; PAGE_SIZE]);
            let err = s.read_page(0, &mut buf).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
        });
    }

    #[test]
    fn reading_past_end_errors_and_leaves_buffer_alone() {
        for_each_backend(|s| {
            s.write_page(0, &filled(1)).unwrap();
            let mut buf = filled(9);
            assert!(s.read_page(1, &mut buf).is_err());
            assert!(s.read_page(100, &mut buf).is_err());
            assert_eq!(buf, filled(9));
        });
    }

    #[test]
    fn write_then_read_round_trips() {
        for_each_backend(|s| {
            s.write_page(0, &patterned(7)).unwrap();
            assert_eq!(read(s, 0), patterned(7));
        });
    }

    #[test]
    fn writing_grows_num_pages() {
        for_each_backend(|s| {
            s.write_page(0, &filled(1)).unwrap();
            assert_eq!(s.num_pages(), 1);
            s.write_page(1, &filled(2)).unwrap();
            assert_eq!(s.num_pages(), 2);
        });
    }

    #[test]
    fn overwriting_does_not_grow_num_pages() {
        for_each_backend(|s| {
            s.write_page(0, &filled(1)).unwrap();
            s.write_page(1, &filled(2)).unwrap();
            s.write_page(0, &filled(3)).unwrap();
            assert_eq!(s.num_pages(), 2);
            assert_eq!(read(s, 0), filled(3));
            assert_eq!(read(s, 1), filled(2));
        });
    }

    #[test]
    fn writing_past_end_fills_gap_with_zeros() {
        for_each_backend(|s| {
            s.write_page(3, &filled(5)).unwrap();
            assert_eq!(s.num_pages(), 4);
            for n in 0..3 {
                assert_eq!(read(s, n), filled(0), "gap page {n}");
            }
            assert_eq!(read(s, 3), filled(5));
        });
    }

    #[test]
    fn pages_are_independent() {
        for_each_backend(|s| {
            for n in 0..10 {
                s.write_page(n, &patterned(n as u8)).unwrap();
            }
            for n in 0..10 {
                assert_eq!(read(s, n), patterned(n as u8), "page {n}");
            }
        });
    }

    #[test]
    fn writes_in_any_order() {
        for_each_backend(|s| {
            for n in [4, 0, 2, 1, 3] {
                s.write_page(n, &patterned(n as u8)).unwrap();
            }
            assert_eq!(s.num_pages(), 5);
            for n in 0..5 {
                assert_eq!(read(s, n), patterned(n as u8), "page {n}");
            }
        });
    }

    #[test]
    fn page_boundaries_are_exact() {
        for_each_backend(|s| {
            let mut first = filled(0xAA);
            first[PAGE_SIZE - 1] = 0x01;
            let mut second = filled(0xBB);
            second[0] = 0x02;
            s.write_page(0, &first).unwrap();
            s.write_page(1, &second).unwrap();
            assert_eq!(read(s, 0), first);
            assert_eq!(read(s, 1), second);
        });
    }

    #[test]
    fn sync_succeeds_and_keeps_data() {
        for_each_backend(|s| {
            s.sync().unwrap();
            s.write_page(0, &patterned(3)).unwrap();
            s.sync().unwrap();
            assert_eq!(read(s, 0), patterned(3));
        });
    }

    #[test]
    fn reads_do_not_change_storage() {
        for_each_backend(|s| {
            s.write_page(0, &patterned(1)).unwrap();
            for _ in 0..3 {
                read(s, 0);
            }
            assert_eq!(s.num_pages(), 1);
            assert_eq!(read(s, 0), patterned(1));
        });
    }
}
