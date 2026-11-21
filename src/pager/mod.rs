//! Page cache on top of any `Storage` backend.

pub mod storage;

pub use storage::{FileStorage, MemoryStorage, PAGE_SIZE, Page, Storage};

use std::io;

pub struct Pager<S: Storage> {
    storage: S,
    /// Cached pages, indexed by page number. `None` = not loaded yet.
    cache: Vec<Option<Box<Page>>>,
    dirty: Vec<bool>,
    num_pages: usize,
}

impl<S: Storage> Pager<S> {
    pub fn new(storage: S) -> Self {
        let num_pages = storage.num_pages();
        Self {
            storage,
            cache: Vec::new(),
            dirty: Vec::new(),
            num_pages,
        }
    }

    /// Read-only access to page `n`. Pages past the end come back zeroed.
    pub fn page(&mut self, n: usize) -> io::Result<&Page> {
        self.load(n)?;
        Ok(self.cache[n].as_deref().unwrap())
    }

    /// Mutable access to page `n`. Marks it dirty; touching a page past the
    /// end grows the database.
    pub fn page_mut(&mut self, n: usize) -> io::Result<&mut Page> {
        self.load(n)?;
        self.dirty[n] = true;
        self.num_pages = self.num_pages.max(n + 1);
        Ok(self.cache[n].as_deref_mut().unwrap())
    }

    /// Write every dirty page back and sync the storage.
    pub fn flush(&mut self) -> io::Result<()> {
        for n in 0..self.cache.len() {
            if self.dirty[n] {
                let page = self.cache[n].as_deref().unwrap();
                self.storage.write_page(n, page)?;
                self.dirty[n] = false;
            }
        }
        self.storage.sync()
    }

    /// Number of pages, including ones created since opening.
    pub fn num_pages(&self) -> usize {
        self.num_pages
    }

    /// Give back the storage. Unflushed changes are dropped.
    #[cfg(test)]
    pub fn into_storage(self) -> S {
        self.storage
    }

    #[cfg(test)]
    fn storage_mut(&mut self) -> &mut S {
        &mut self.storage
    }

    /// Make sure page `n` is in the cache.
    fn load(&mut self, n: usize) -> io::Result<()> {
        if n >= self.cache.len() {
            self.cache.resize_with(n + 1, || None);
            self.dirty.resize(n + 1, false);
        }
        if self.cache[n].is_none() {
            let mut page = Box::new([0; PAGE_SIZE]);
            if n < self.storage.num_pages() {
                self.storage.read_page(n, &mut page)?;
            }
            self.cache[n] = Some(page);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_pages_are_zeroed() {
        let mut pager = Pager::new(MemoryStorage::new());
        assert_eq!(pager.page(3).unwrap(), &[0; PAGE_SIZE]);
        assert_eq!(pager.num_pages(), 0, "reading doesn't grow the database");
    }

    #[test]
    fn writes_survive_flush_in_memory() {
        let mut pager = Pager::new(MemoryStorage::new());
        pager.page_mut(2).unwrap()[..5].copy_from_slice(b"hello");
        assert_eq!(pager.num_pages(), 3);
        pager.flush().unwrap();

        let mut pager = Pager::new(pager.into_storage());
        assert_eq!(&pager.page(2).unwrap()[..5], b"hello");
        assert_eq!(pager.num_pages(), 3);
    }

    #[test]
    fn unflushed_writes_are_lost() {
        let mut pager = Pager::new(MemoryStorage::new());
        pager.page_mut(0).unwrap()[0] = 42;

        let mut pager = Pager::new(pager.into_storage());
        assert_eq!(pager.page(0).unwrap()[0], 0);
    }

    #[test]
    fn writes_survive_reopen_on_disk() {
        let path = TempPath::new();

        let mut pager = Pager::new(FileStorage::open(&path).unwrap());
        pager.page_mut(1).unwrap()[..5].copy_from_slice(b"world");
        pager.flush().unwrap();
        drop(pager);

        let mut pager = Pager::new(FileStorage::open(&path).unwrap());
        assert_eq!(pager.num_pages(), 2);
        assert_eq!(&pager.page(1).unwrap()[..5], b"world");
    }

    use storage::test_util::{TempPath, filled, patterned};

    /// Wraps `MemoryStorage`, records every call, and can be told to fail.
    #[derive(Default)]
    struct Spy {
        inner: MemoryStorage,
        reads: Vec<usize>,
        writes: Vec<usize>,
        syncs: usize,
        fail_reads: bool,
        fail_writes: bool,
        fail_sync: bool,
    }

    impl Spy {
        /// A spy that already holds `pages`, with the call log cleared.
        fn with_pages(pages: &[Box<Page>]) -> Self {
            let mut spy = Spy::default();
            for (n, page) in pages.iter().enumerate() {
                spy.inner.write_page(n, page).unwrap();
            }
            spy
        }

        fn clear_log(&mut self) {
            self.reads.clear();
            self.writes.clear();
            self.syncs = 0;
        }
    }

    fn injected() -> io::Error {
        io::Error::other("injected failure")
    }

    impl Storage for Spy {
        fn read_page(&mut self, n: usize, buf: &mut Page) -> io::Result<()> {
            self.reads.push(n);
            if self.fail_reads {
                return Err(injected());
            }
            self.inner.read_page(n, buf)
        }

        fn write_page(&mut self, n: usize, buf: &Page) -> io::Result<()> {
            self.writes.push(n);
            if self.fail_writes {
                return Err(injected());
            }
            self.inner.write_page(n, buf)
        }

        fn num_pages(&self) -> usize {
            self.inner.num_pages()
        }

        fn sync(&mut self) -> io::Result<()> {
            self.syncs += 1;
            if self.fail_sync {
                return Err(injected());
            }
            Ok(())
        }
    }

    // ---- opening ----------------------------------------------------------

    #[test]
    fn num_pages_starts_from_storage() {
        let pager = Pager::new(Spy::with_pages(&[filled(1), filled(2), filled(3)]));
        assert_eq!(pager.num_pages(), 3);
    }

    #[test]
    fn new_does_not_touch_storage() {
        let pager = Pager::new(Spy::with_pages(&[filled(1)]));
        let spy = pager.into_storage();
        assert!(spy.reads.is_empty());
        assert!(spy.writes.is_empty());
        assert_eq!(spy.syncs, 0);
    }

    #[test]
    fn existing_pages_are_loaded() {
        let mut pager = Pager::new(Spy::with_pages(&[patterned(1), patterned(2)]));
        assert_eq!(pager.page(0).unwrap(), &*patterned(1));
        assert_eq!(pager.page(1).unwrap(), &*patterned(2));
    }

    // ---- caching ----------------------------------------------------------

    #[test]
    fn each_page_is_read_from_storage_once() {
        let mut pager = Pager::new(Spy::with_pages(&[filled(1), filled(2)]));
        for _ in 0..3 {
            pager.page(0).unwrap();
            pager.page_mut(1).unwrap();
        }
        let mut reads = pager.into_storage().reads;
        reads.sort();
        assert_eq!(reads, vec![0, 1]);
    }

    #[test]
    fn pages_past_end_are_not_read_from_storage() {
        let mut pager = Pager::new(Spy::with_pages(&[filled(1)]));
        pager.page(5).unwrap();
        pager.page_mut(6).unwrap();
        assert!(pager.into_storage().reads.is_empty());
    }

    #[test]
    fn changes_are_visible_before_flush() {
        let mut pager = Pager::new(MemoryStorage::new());
        pager.page_mut(0).unwrap()[10] = 99;
        assert_eq!(pager.page(0).unwrap()[10], 99);
    }

    #[test]
    fn changes_keep_rest_of_existing_page() {
        let mut pager = Pager::new(Spy::with_pages(&[patterned(4)]));
        pager.page_mut(0).unwrap()[0] = 0xFF;
        pager.flush().unwrap();

        let mut expected = patterned(4);
        expected[0] = 0xFF;
        let mut pager = Pager::new(pager.into_storage());
        assert_eq!(pager.page(0).unwrap(), &*expected);
    }

    #[test]
    fn pages_are_independent() {
        let mut pager = Pager::new(MemoryStorage::new());
        for n in 0..20 {
            pager
                .page_mut(n)
                .unwrap()
                .copy_from_slice(&patterned(n as u8)[..]);
        }
        for n in 0..20 {
            assert_eq!(pager.page(n).unwrap(), &*patterned(n as u8), "page {n}");
        }
    }

    // ---- num_pages --------------------------------------------------------

    #[test]
    fn page_mut_past_end_grows_to_cover_it() {
        let mut pager = Pager::new(MemoryStorage::new());
        pager.page_mut(4).unwrap();
        assert_eq!(pager.num_pages(), 5);
        pager.page_mut(2).unwrap();
        assert_eq!(
            pager.num_pages(),
            5,
            "touching an earlier page doesn't shrink it"
        );
    }

    #[test]
    fn page_mut_on_existing_page_keeps_num_pages() {
        let mut pager = Pager::new(Spy::with_pages(&[filled(1), filled(2)]));
        pager.page_mut(0).unwrap();
        assert_eq!(pager.num_pages(), 2);
    }

    #[test]
    fn reading_past_end_keeps_num_pages() {
        let mut pager = Pager::new(Spy::with_pages(&[filled(1)]));
        pager.page(10).unwrap();
        assert_eq!(pager.num_pages(), 1);
    }

    // ---- flushing ---------------------------------------------------------

    #[test]
    fn flush_writes_only_dirty_pages() {
        let mut pager = Pager::new(Spy::with_pages(&[
            filled(0),
            filled(1),
            filled(2),
            filled(3),
        ]));
        pager.page(0).unwrap();
        pager.page_mut(1).unwrap();
        pager.page(2).unwrap();
        pager.page_mut(3).unwrap();
        pager.flush().unwrap();
        assert_eq!(pager.into_storage().writes, vec![1, 3]);
    }

    #[test]
    fn flush_with_nothing_dirty_only_syncs() {
        let mut pager = Pager::new(Spy::with_pages(&[filled(1)]));
        pager.page(0).unwrap();
        pager.flush().unwrap();
        let spy = pager.into_storage();
        assert!(spy.writes.is_empty());
        assert_eq!(spy.syncs, 1);
    }

    #[test]
    fn flush_syncs_once_after_writing() {
        let mut pager = Pager::new(Spy::default());
        for n in 0..5 {
            pager.page_mut(n).unwrap();
        }
        pager.flush().unwrap();
        let spy = pager.into_storage();
        assert_eq!(spy.writes.len(), 5);
        assert_eq!(spy.syncs, 1);
    }

    #[test]
    fn second_flush_writes_nothing() {
        let mut pager = Pager::new(Spy::default());
        pager.page_mut(0).unwrap();
        pager.flush().unwrap();
        pager.storage_mut().clear_log();
        pager.flush().unwrap();
        assert!(pager.into_storage().writes.is_empty());
    }

    #[test]
    fn page_dirtied_again_after_flush_is_written_again() {
        let mut pager = Pager::new(Spy::default());
        pager.page_mut(0).unwrap()[0] = 1;
        pager.flush().unwrap();
        pager.storage_mut().clear_log();

        pager.page_mut(0).unwrap()[0] = 2;
        pager.flush().unwrap();
        let spy = pager.into_storage();
        assert_eq!(spy.writes, vec![0]);

        let mut pager = Pager::new(spy);
        assert_eq!(pager.page(0).unwrap()[0], 2);
    }

    #[test]
    fn flushing_a_far_page_fills_the_gap_with_zeros() {
        let mut pager = Pager::new(MemoryStorage::new());
        pager.page_mut(5).unwrap().copy_from_slice(&filled(7)[..]);
        pager.flush().unwrap();

        let storage = pager.into_storage();
        assert_eq!(storage.num_pages(), 6);
        let mut pager = Pager::new(storage);
        for n in 0..5 {
            assert_eq!(pager.page(n).unwrap(), &*filled(0), "gap page {n}");
        }
        assert_eq!(pager.page(5).unwrap(), &*filled(7));
    }

    #[test]
    fn flushed_data_is_readable_by_a_new_pager() {
        let path = TempPath::new();
        {
            let mut pager = Pager::new(FileStorage::open(&path).unwrap());
            for n in 0..50 {
                pager
                    .page_mut(n)
                    .unwrap()
                    .copy_from_slice(&patterned(n as u8)[..]);
            }
            pager.flush().unwrap();
        }
        let mut pager = Pager::new(FileStorage::open(&path).unwrap());
        assert_eq!(pager.num_pages(), 50);
        for n in 0..50 {
            assert_eq!(pager.page(n).unwrap(), &*patterned(n as u8), "page {n}");
        }
    }

    // ---- errors -----------------------------------------------------------

    #[test]
    fn read_errors_are_returned() {
        let mut spy = Spy::with_pages(&[filled(1)]);
        spy.fail_reads = true;
        let mut pager = Pager::new(spy);
        assert!(pager.page(0).is_err());
        assert!(pager.page_mut(0).is_err());
    }

    #[test]
    fn failed_read_is_retried_next_time() {
        let mut spy = Spy::with_pages(&[patterned(3)]);
        spy.fail_reads = true;
        let mut pager = Pager::new(spy);
        assert!(pager.page(0).is_err());

        pager.storage_mut().fail_reads = false;
        assert_eq!(pager.page(0).unwrap(), &*patterned(3));
    }

    #[test]
    fn write_error_keeps_page_dirty_for_retry() {
        let mut pager = Pager::new(Spy::default());
        pager.page_mut(0).unwrap()[0] = 42;
        pager.storage_mut().fail_writes = true;
        assert!(pager.flush().is_err());

        pager.storage_mut().fail_writes = false;
        pager.storage_mut().clear_log();
        pager.flush().unwrap();
        let spy = pager.into_storage();
        assert_eq!(spy.writes, vec![0]);

        let mut pager = Pager::new(spy);
        assert_eq!(pager.page(0).unwrap()[0], 42);
    }

    #[test]
    fn sync_errors_are_returned() {
        let mut pager = Pager::new(Spy::default());
        pager.page_mut(0).unwrap();
        pager.storage_mut().fail_sync = true;
        assert!(pager.flush().is_err());
    }
}
