use std::io;

use super::{PAGE_SIZE, Page, Storage, out_of_range};

/// Pages kept in a `Vec`. Used for tests now and the browser build later.
#[derive(Default)]
pub struct MemoryStorage {
    pages: Vec<Box<Page>>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Storage for MemoryStorage {
    fn read_page(&mut self, n: usize, buf: &mut Page) -> io::Result<()> {
        let page = self
            .pages
            .get(n)
            .ok_or_else(|| out_of_range(n, self.pages.len()))?;
        buf.copy_from_slice(&page[..]);
        Ok(())
    }

    fn write_page(&mut self, n: usize, buf: &Page) -> io::Result<()> {
        if n >= self.pages.len() {
            self.pages.resize_with(n + 1, || Box::new([0; PAGE_SIZE]));
        }
        self.pages[n].copy_from_slice(buf);
        Ok(())
    }

    fn num_pages(&self) -> usize {
        self.pages.len()
    }

    fn sync(&mut self) -> io::Result<()> {
        Ok(())
    }
}
