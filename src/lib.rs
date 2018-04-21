
#[macro_use]
extern crate bitflags;

use std::io::*;
use std::fs::File;
use std::path::PathBuf;

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

struct GtkIconCache {
    // header
    hash_offset: CARD32,
    directory_list_offset: CARD32,

    // hash
    n_buckets: CARD32,

    reader: BufReader<File>,
}

struct GtkIconImage {
    directory_index: CARD16,
    flags: GtkIconFlag,
    image_data_offset: CARD32,
}

trait IconCacheReadHelper {
    fn read16(&mut self) -> Result<CARD16>;

    fn read32(&mut self) -> Result<CARD32>;

    fn read_icon_flag(&mut self) -> Result<GtkIconFlag>;

    fn read_image(&mut self) -> Result<GtkIconImage>;
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
        rdr.seek(SeekFrom::Start(directory_list_offset as u64))?;
        let n_directorys = rdr.read32()?;

        // dump directories
        for i in 0..n_directorys {
            rdr.seek(SeekFrom::Start((directory_list_offset + 4 + 4 * i) as u64))?;
            let offset = rdr.read32()?;

            if let Ok(_) = rdr.seek(SeekFrom::Start(offset as u64)) {
                let mut buf = vec![];
                rdr.read_until(b'\0', &mut buf)?;
                println!("{}", String::from_utf8_lossy(&buf[..]));
            }
        }

        Ok(GtkIconCache {
            hash_offset,
            directory_list_offset,

            n_buckets: 0,

            reader: rdr,
        })
    }
}

#[cfg(test)]
mod test {

    use GtkIconCache;

    #[test]
    fn test_icon_cache() {
        let path = "/usr/share/icons/Flattr/icon-theme.cache".parse().unwrap();
        let icon_cache = GtkIconCache::with_file_path(path).unwrap();

        println!("{:?}", icon_cache.hash_offset);
    }
}
