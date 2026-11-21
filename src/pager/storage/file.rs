use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

use super::{PAGE_SIZE, Page, Storage, out_of_range};

/// Pages stored in a file: page `n` lives at byte offset `n * PAGE_SIZE`.
pub struct FileStorage {
    file: File,
    num_pages: usize,
}

impl FileStorage {
    /// Open the file at `path`, creating it if it doesn't exist.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        let len = file.metadata()?.len() as usize;
        if len % PAGE_SIZE != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("file size {len} is not a multiple of {PAGE_SIZE}"),
            ));
        }
        Ok(Self {
            file,
            num_pages: len / PAGE_SIZE,
        })
    }

    fn seek_to(&mut self, n: usize) -> io::Result<()> {
        self.file.seek(SeekFrom::Start((n * PAGE_SIZE) as u64))?;
        Ok(())
    }
}

impl Storage for FileStorage {
    fn read_page(&mut self, n: usize, buf: &mut Page) -> io::Result<()> {
        if n >= self.num_pages {
            return Err(out_of_range(n, self.num_pages));
        }
        self.seek_to(n)?;
        self.file.read_exact(buf)
    }

    fn write_page(&mut self, n: usize, buf: &Page) -> io::Result<()> {
        self.seek_to(n)?;
        self.file.write_all(buf)?;
        self.num_pages = self.num_pages.max(n + 1);
        Ok(())
    }

    fn num_pages(&self) -> usize {
        self.num_pages
    }

    fn sync(&mut self) -> io::Result<()> {
        self.file.sync_all()
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_util::{TempPath, filled, patterned};
    use super::*;

    fn read(storage: &mut FileStorage, n: usize) -> Box<Page> {
        let mut buf = Box::new([0; PAGE_SIZE]);
        storage.read_page(n, &mut buf).unwrap();
        buf
    }

    #[test]
    fn open_creates_missing_file() {
        let path = TempPath::new();
        assert!(!path.as_ref().exists());
        let storage = FileStorage::open(&path).unwrap();
        assert!(path.as_ref().exists());
        assert_eq!(storage.num_pages(), 0);
    }

    #[test]
    fn open_fails_in_missing_directory() {
        let path = std::env::temp_dir().join("sqlite_clone_no_such_dir/x.db");
        assert!(FileStorage::open(path).is_err());
    }

    #[test]
    fn data_survives_reopen() {
        let path = TempPath::new();
        {
            let mut storage = FileStorage::open(&path).unwrap();
            for n in 0..3 {
                storage.write_page(n, &patterned(n as u8)).unwrap();
            }
            storage.sync().unwrap();
        }
        let mut storage = FileStorage::open(&path).unwrap();
        assert_eq!(storage.num_pages(), 3);
        for n in 0..3 {
            assert_eq!(read(&mut storage, n), patterned(n as u8), "page {n}");
        }
    }

    #[test]
    fn reopening_does_not_truncate() {
        let path = TempPath::new();
        FileStorage::open(&path)
            .unwrap()
            .write_page(0, &filled(4))
            .unwrap();
        drop(FileStorage::open(&path).unwrap());
        let mut storage = FileStorage::open(&path).unwrap();
        assert_eq!(storage.num_pages(), 1);
        assert_eq!(read(&mut storage, 0), filled(4));
    }

    #[test]
    fn file_length_is_pages_times_page_size() {
        let path = TempPath::new();
        let mut storage = FileStorage::open(&path).unwrap();
        storage.write_page(2, &filled(1)).unwrap();
        storage.sync().unwrap();
        let len = std::fs::metadata(&path).unwrap().len();
        assert_eq!(len, 3 * PAGE_SIZE as u64);
    }

    #[test]
    fn page_n_is_at_offset_n_times_page_size() {
        let path = TempPath::new();
        let mut storage = FileStorage::open(&path).unwrap();
        storage.write_page(0, &filled(0x11)).unwrap();
        storage.write_page(1, &patterned(9)).unwrap();
        storage.sync().unwrap();

        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..PAGE_SIZE], &filled(0x11)[..]);
        assert_eq!(&bytes[PAGE_SIZE..2 * PAGE_SIZE], &patterned(9)[..]);
    }

    #[test]
    fn reads_pages_written_by_another_program() {
        let path = TempPath::new();
        let mut bytes = vec![0x22; PAGE_SIZE];
        bytes.extend_from_slice(&patterned(5)[..]);
        std::fs::write(&path, bytes).unwrap();

        let mut storage = FileStorage::open(&path).unwrap();
        assert_eq!(storage.num_pages(), 2);
        assert_eq!(read(&mut storage, 0), filled(0x22));
        assert_eq!(read(&mut storage, 1), patterned(5));
    }

    #[test]
    fn rejects_file_that_is_not_whole_pages() {
        for len in [1, PAGE_SIZE - 1, PAGE_SIZE + 1, 3 * PAGE_SIZE - 7] {
            let path = TempPath::new();
            std::fs::write(&path, vec![0; len]).unwrap();
            let err = FileStorage::open(&path).err().expect("should reject");
            assert_eq!(err.kind(), io::ErrorKind::InvalidData, "len {len}");
        }
    }

    #[test]
    fn accepts_empty_existing_file() {
        let path = TempPath::new();
        std::fs::write(&path, b"").unwrap();
        assert_eq!(FileStorage::open(&path).unwrap().num_pages(), 0);
    }
}
