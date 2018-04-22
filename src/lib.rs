
#[macro_use]
extern crate bitflags;

use std::io;
use std::io::{SeekFrom, ErrorKind, Result, BufRead, Error, Read, BufReader};
use std::num::Wrapping;
use std::fs::File;
use std::path::PathBuf;
use std::collections::{HashMap, HashSet};

type CARD16 = u16;
type CARD32 = u32;

bitflags! {
    struct GtkIconFlag: CARD16 {
        const HAS_SUFFIX_PNG = 0b00000001;
        const HAS_SUFFIX_XPM = 0b00000010;
        const HAS_SUFFIX_SVG = 0b00000100;
        const HAS_ICON_FILE  = 0b00001000;
    }
}

pub struct GtkIconCache {
    hash_offset: CARD32,
    directory_list_offset: CARD32,

    n_directorys: CARD32,
    n_buckets: CARD32,

    dir_names: HashMap<CARD32, String>,
    reader: BufReader<File>,
}

struct GtkIconImage {
    directory_index: CARD16,
    flags: GtkIconFlag,
    image_data_offset: CARD32,
}

trait IconCacheReadHelper {
    fn read16(&mut self) -> Result<CARD16>;
    fn read16_from(&mut self, offset: u64) -> Result<CARD16> {
        self.seek(offset).and_then(|_| self.read16())
    }

    fn read32(&mut self) -> Result<CARD32>;
    fn read32_from(&mut self, offset: u64) -> Result<CARD32> {
        self.seek(offset).and_then(|_| self.read32())
    }

    fn read_icon_flag(&mut self) -> Result<GtkIconFlag>;

    fn read_image(&mut self) -> Result<GtkIconImage>;

    fn read_cstring(&mut self) -> Result<String>;
    fn read_cstring_from(&mut self, offset: u64) -> Result<String> {
        self.seek(offset).and_then(|_| self.read_cstring())
    }

    fn seek(&mut self, offset: u64) -> Result<u64>;
}

impl IconCacheReadHelper for BufReader<File> {
    fn read16(&mut self) -> Result<CARD16> {
        let mut buf16 = [0; 2];

        self.read_exact(&mut buf16)?;

        Ok((buf16[0] as CARD16) << 8 | buf16[1] as CARD16)
    }

    fn read32(&mut self) -> Result<CARD32> {
        let mut buf32 = [0; 4];

        self.read_exact(&mut buf32)?;

        Ok((buf32[0] as CARD32) << 24 |
           (buf32[1] as CARD32) << 16 |
           (buf32[2] as CARD32) <<  8 |
           (buf32[3] as CARD32))
    }

    fn read_icon_flag(&mut self) -> Result<GtkIconFlag> {
        let flag = self.read16()?;

        Ok(GtkIconFlag::from_bits(flag).unwrap())
    }

    fn read_image(&mut self) -> Result<GtkIconImage> {
        Ok(GtkIconImage {
            directory_index: self.read16()?,
            flags: self.read_icon_flag()?,
            image_data_offset: self.read32()?,
        })
    }

    fn read_cstring(&mut self) -> Result<String> {
        let mut buf = vec![];
        self.read_until(b'\0', &mut buf)?;
        Ok(String::from_utf8_lossy(&buf[0..buf.len() - 1]).to_string())
    }

    fn seek(&mut self, offset: u64) -> Result<u64> {
        io::Seek::seek(self, SeekFrom::Start(offset))
    }
}

impl GtkIconCache {
    pub fn with_file_path(path: PathBuf) -> Result<Self> {
        // read data
        let f = File::open(&path)?;
        let last_modified = f.metadata().and_then(|x| x.modified()).ok();
        let mut rdr = BufReader::new(f);

        let major_version = rdr.read16()?;
        let _minor_version = rdr.read16()?;
        if major_version != 1 {
            return Err(Error::new(ErrorKind::Other, "major_version not supported."));
        }

        let hash_offset = rdr.read32()?;
        let directory_list_offset = rdr.read32()?;

        // directory list
        let n_directorys = rdr.read32_from(directory_list_offset as u64)?;

        // dump directories
        let mut dir_names = HashMap::new();
        for i in 0..n_directorys {
            let offset = rdr.read32_from((directory_list_offset + 4 + 4 * i) as u64)?;
            if let Ok(dir) = rdr.read_cstring_from(offset as u64) {
                dir_names.insert(offset, dir);
            }
        }

        // hash bucket count
        rdr.seek(hash_offset as u64)?;
        let n_buckets = rdr.read32()?;

        Ok(GtkIconCache {
            hash_offset,
            directory_list_offset,

            n_directorys,
            n_buckets,

            dir_names,
            reader: rdr,
        })
    }

    pub fn lookup<T: AsRef<str>>(&mut self, name: T) -> Result<Vec<&String>> {
        let icon_hash = icon_name_hash(name.as_ref());
        let bucket_index = icon_hash % self.n_buckets;
        let offset = self.hash_offset + 4 + bucket_index * 4;

        let mut bucket_offset = self.reader.read32_from(offset as u64)?;
        while let Ok(bucket_name) = self.reader.read32_from(Wrapping(bucket_offset as u64 + 4).0) {
            // read bucket name
            if let Ok(cache) = self.reader.read_cstring_from(bucket_name as u64) {
                if cache == name.as_ref() {
                    let list_offset = bucket_offset + 8;
                    let list_len = self.reader.read32_from(list_offset as u64)?;

                    let mut r = HashSet::with_capacity(list_len as usize);
                    // read cached dirs
                    for i in 0..list_len {
                        if let Ok(dir_index) = self.reader.read16_from((list_offset + 4 + 8 * i) as u64) {
                            if let Ok(offset) = self.reader.read32_from(self.directory_list_offset as u64 + 4 + (dir_index as u64) * 4) {
                                r.insert(offset);
                            }
                        }
                    }

                    let ref dir_names = self.dir_names;
                    return Ok(r.iter().map(|x| dir_names.get(&x).unwrap()).collect())
                }
            }

            // find in next bucket
            bucket_offset = self.reader.read32_from(bucket_offset as u64)?;
        }

        // not found
        Ok(vec![])
    }
}

fn icon_name_hash<T: AsRef<str>>(name: T) -> u32 {
    name.as_ref().as_bytes().iter().fold(Wrapping(0), |r, &c| (r << 5) - r + Wrapping(c as u32)).0
}

#[cfg(test)]
mod test {

    use GtkIconCache;
    use icon_name_hash;

    #[test]
    fn test_icon_cache() {
        let path = "/usr/share/icons/Flattr/icon-theme.cache".parse().unwrap();
        let mut icon_cache = GtkIconCache::with_file_path(path).unwrap();

        let icon_name = "firefox";
        let icon_hash = icon_name_hash(icon_name);

        println!("{:?}", icon_hash);
        println!("{:?}", icon_cache.hash_offset);
        println!("{:?}", icon_cache.lookup("firefox"));
    }

    #[test]
    fn test_icon_name_hash() {
        assert_eq!(
            icon_name_hash("firefox") % 12,
            icon_name_hash("image-generic") % 12
        );
    }
}
