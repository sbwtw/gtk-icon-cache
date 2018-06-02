//!
//! This crate provide a reader for gtk-icon-cache file.
//!
//! ```
//! use gtk_icon_cache::*;
//!
//! let path = "test/caches/icon-theme.cache";
//! let icon_cache = GtkIconCache::with_file_path(path).unwrap();
//!
//! // lookup for `firefox`
//! let dirs = icon_cache.lookup("firefox").unwrap();
//!
//! // icon should be found in apps/64
//! assert!(dirs.contains(&&"apps/64".to_string()));
//! ```
//!
//! _See_:
//! - [GTK icon-cache specific](https://github.com/GNOME/gtk/blob/master/docs/iconcache.txt)
//! - [Qt icon loader](https://codereview.qt-project.org/#/c/125379/9/src/gui/image/qiconloader.cpp)
//!

extern crate memmap;
#[macro_use]
extern crate log;

use memmap::Mmap;

use std::io::{ErrorKind, Result, Error};
use std::num::Wrapping;
use std::fs::File;
use std::path::Path;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

///
/// GtkIconCache
///
#[derive(Debug, Clone)]
pub struct GtkIconCache {
    hash_offset: usize,
    directory_list_offset: usize,

    n_buckets: usize,

    dir_names: HashMap<usize, String>,
    file_mmap: Arc<Mmap>,
}

impl GtkIconCache {
    ///
    /// Create with a cache file.
    ///
    /// * `path` - Cache file path.
    ///
    pub fn with_file_path<T: AsRef<Path>>(path: T) -> Result<Self> {
        // read data
        let f = File::open(&path.as_ref())?;
        let _last_modified = f.metadata().and_then(|x| x.modified()).ok();
        let mmap = unsafe { Mmap::map(&f)? };

        let r = Self {
            hash_offset: 0,
            directory_list_offset: 0,

            n_buckets: 0,

            dir_names: HashMap::new(),
            file_mmap: Arc::new(mmap),
        };

        match r.load_cache() {
            Some(cache) => Ok(cache),
            _ => Err(Error::new(ErrorKind::Other, "cache load failed.")),
        }
    }

    fn load_cache(mut self) -> Option<Self> {

        let major_version = self.read_card16_from(0)?;
        let minor_version = self.read_card16_from(2)?;

        self.hash_offset = self.read_card32_from(4)?;
        self.directory_list_offset = self.read_card32_from(8)?;
        self.n_buckets = self.read_card32_from(self.hash_offset)?;

        if major_version != 1usize && minor_version != 0usize {
            return None;
        }

        let n_directorys = self.read_card32_from(self.directory_list_offset)?;

        // dump directories
        for i in 0..n_directorys {
            let offset = self.read_card32_from(self.directory_list_offset + 4 + 4 * i)?;
            if let Some(dir) = self.read_cstring_from(offset as usize) {
                self.dir_names.insert(offset, dir);
            }
        }

        trace!("{:#?}", self);

        Some(self)
    }

    fn read_card16_from(&self, offset: usize) -> Option<usize> {
        let m = &self.file_mmap;

        if offset < self.file_mmap.len() - 2 {
            Some((m[offset    ] as usize) << 8 |
                 (m[offset + 1] as usize))
        } else {
            None
        }
    }

    fn read_card32_from(&self, offset: usize) -> Option<usize> {
        let m = &self.file_mmap;

        if offset > 0 && offset < self.file_mmap.len() - 4 {
            Some((m[offset    ] as usize) << 24 |
                 (m[offset + 1] as usize) << 16 |
                 (m[offset + 2] as usize) <<  8 |
                 (m[offset + 3] as usize))
        } else {
            None
        }
    }

    fn read_cstring_from(&self, offset: usize) -> Option<String> {
        let mut terminate = offset;

        while self.file_mmap[terminate] != b'\0' { terminate += 1; }

        if terminate == offset { return None; }

        Some(String::from_utf8_lossy(&self.file_mmap[offset..terminate]).to_string())
    }

    ///
    /// Look up an icon.
    ///
    /// * `name` - icon name.
    ///
    pub fn lookup<T: AsRef<str>>(&self, name: T) -> Option<Vec<&String>> {
        let icon_hash = icon_name_hash(name.as_ref());
        let bucket_index = icon_hash % self.n_buckets;

        let mut bucket_offset = self.read_card32_from(self.hash_offset + 4 + bucket_index * 4)?;
        while let Some(bucket_name_offset) = self.read_card32_from(bucket_offset + 4) {
            // read bucket name
            if let Some(cache) = self.read_cstring_from(bucket_name_offset) {
                if cache == name.as_ref() {
                    let list_offset = self.read_card32_from(bucket_offset + 8)?;
                    let list_len = self.read_card32_from(list_offset)?;

                    let mut r = HashSet::with_capacity(list_len);
                    // read cached dirs
                    for i in 0..list_len {
                        if let Some(dir_index) = self.read_card16_from(list_offset + 4 + 8 * i) {
                            if let Some(offset) = self.read_card32_from(self.directory_list_offset + 4 + dir_index * 4) {
                                r.insert(offset);
                            }
                        }
                    }

                    let ref dir_names = self.dir_names;
                    return Some(r.iter().map(|x| dir_names.get(&x).unwrap()).collect())
                }
            }

            // find in next
            bucket_offset = self.read_card32_from(bucket_offset)?;
        }

        // not found
        None
    }
}

fn icon_name_hash<T: AsRef<str>>(name: T) -> usize {

    let name = name.as_ref().as_bytes();

    name.iter()
        .fold(Wrapping(0u32), |r, &c| (r << 5) - r + Wrapping(c as u32)).0
        as usize
}

#[cfg(test)]
mod test {

    use GtkIconCache;
    use icon_name_hash;

    #[test]
    fn test_icon_cache() {
        let path = "test/caches/icon-theme.cache";
        let icon_cache = GtkIconCache::with_file_path(path).unwrap();

        let icon_name = "web-browser";
        let icon_hash = icon_name_hash(icon_name);

        assert_eq!(icon_hash, 2769241519);
        assert_eq!(icon_cache.hash_offset, 12);

        println!("=> {:?}", icon_cache.lookup(icon_name));
    }

    #[test]
    fn test_cache_test1() {
        let path = "test/caches/test1.cache";
        let icon_cache = GtkIconCache::with_file_path(path).unwrap();

        let dirs = icon_cache.lookup("test").unwrap();
        assert!(dirs.contains(&&"apps/32".to_string()));
        assert!(dirs.contains(&&"apps/48".to_string()));

        let dirs = icon_cache.lookup("deepin-deb-installer").unwrap();
        assert!(dirs.contains(&&"apps/16".to_string()));
        assert!(dirs.contains(&&"apps/32".to_string()));
        assert!(dirs.contains(&&"apps/48".to_string()));
        assert!(dirs.contains(&&"apps/scalable".to_string()));
    }

    #[test]
    fn test_icon_name_hash() {
        assert_eq!(icon_name_hash("deepin-deb-installer"), 1927089920);
    }
}
