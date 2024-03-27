use std::{fs::Metadata, io::Write};

use cfg_if::cfg_if;

#[derive(Debug, Clone, Default)]
pub struct ExtraFields {
    pub values: Vec<ExtraField>,
}

impl ExtraFields {
    pub fn data_length(&self) -> u16 {
        self.values.iter().map(|f| 4 + f.field_size()).sum()
    }

    pub fn new_from_fs(metadata: &Metadata) -> Self {
        Self {
            values: ExtraField::new_from_fs(metadata).into_iter().collect(),
        }
    }
}

// Other headers are not used and are simply ignored
#[derive(Debug, Clone, Copy)]
pub enum ExtraField {
    Ntfs {
        mtime: u64,
        atime: u64,
        ctime: u64,
    },
    // Variable length data field is unused
    Unix {
        atime: u32,
        mtime: u32,
        uid: u16,
        gid: u16,
    },
}

impl ExtraField {
    #[inline]
    fn header_id(&self) -> u16 {
        match self {
            Self::Ntfs {
                mtime: _,
                atime: _,
                ctime: _,
            } => 0x000a,
            Self::Unix {
                atime: _,
                mtime: _,
                uid: _,
                gid: _,
            } => 0x000d,
        }
    }

    #[inline]
    fn field_size(&self) -> u16 {
        match self {
            Self::Ntfs {
                mtime: _,
                atime: _,
                ctime: _,
            } => 32,
            Self::Unix {
                atime: _,
                mtime: _,
                uid: _,
                gid: _,
            } => 12,
        }
    }

    pub fn write<W: Write>(self, writer: &mut W) -> std::io::Result<()> {
        // Header ID
        writer.write_all(&self.header_id().to_le_bytes())?;
        // Field data size
        writer.write_all(&self.field_size().to_le_bytes())?;

        match self {
            Self::Ntfs {
                mtime,
                atime,
                ctime,
            } => {
                // Reserved field
                writer.write_all(&0_u32.to_le_bytes())?;

                // Tag1 number
                writer.write_all(&1_u16.to_le_bytes())?;
                // Tag1 size
                writer.write_all(&24_u16.to_le_bytes())?;

                // Mtime
                writer.write_all(&mtime.to_le_bytes())?;
                // Atime
                writer.write_all(&atime.to_le_bytes())?;
                // Ctime
                writer.write_all(&ctime.to_le_bytes())?;
            }
            Self::Unix {
                atime,
                mtime,
                uid,
                gid,
            } => {
                // Atime
                writer.write_all(&atime.to_le_bytes())?;
                // Mtime
                writer.write_all(&mtime.to_le_bytes())?;
                // Uid
                writer.write_all(&uid.to_le_bytes())?;
                // Gid
                writer.write_all(&gid.to_le_bytes())?;
            }
        }

        Ok(())
    }

    #[inline]
    pub fn new_from_fs(metadata: &Metadata) -> Option<Self> {
        cfg_if! {
            if #[cfg(target_os = "windows")] {
                Some(Self::new_windows(metadata))
            } else if #[cfg(target_os = "linux")] {
                Some(Self::new_linux(metadata))
            } else if #[cfg(target_os = "unix")] {
                Some(Self::new_unix(metadata))
            } else {
                None
            }
        }
    }

    /// Due to differences between the data size rust API provides and what ZIP uses, some debug
    /// mode runtime assertions are being made to make sure that the values lay in the sane region.
    /// In the release build, panicking conversion is used for atime and mtime and higher part of
    /// UID and GID is cut off.
    #[cfg(target_os = "linux")]
    fn new_linux(metadata: &Metadata) -> Self {
        use std::os::linux::fs::MetadataExt;

        debug_assert!(!metadata.st_atime().is_negative());
        debug_assert!(metadata.st_atime() < u32::MAX.into());
        let atime = metadata.st_atime().try_into().unwrap();

        debug_assert!(!metadata.st_mtime().is_negative());
        debug_assert!(metadata.st_mtime() < u32::MAX.into());
        let mtime = metadata.st_mtime().try_into().unwrap();

        debug_assert!(metadata.st_uid() <= u16::MAX.into());
        let uid = (metadata.st_uid() & 0xFFFF) as u16;

        debug_assert!(metadata.st_gid() <= u16::MAX.into());
        let gid = (metadata.st_gid() & 0xFFFF) as u16;

        Self::Unix {
            atime,
            mtime,
            uid,
            gid,
        }
    }

    /// Due to differences between the data size rust API provides and what ZIP uses, some debug
    /// mode runtime assertions are being made to make sure that the values lay in the sane region.
    /// In the release build, panicking conversion is used for atime and mtime and higher part of
    /// UID and GID is cut off.
    #[cfg(target_os = "unix")]
    fn new_unix(metadata: &Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;

        debug_assert!(!metadata.st_atime().is_negative());
        debug_assert!(metadata.st_atime() < u32::MAX.into());
        let atime = metadata.st_atime().try_into().unwrap();

        debug_assert!(!metadata.st_mtime().is_negative());
        debug_assert!(metadata.st_mtime() < u32::MAX.into());
        let mtime = metadata.st_mtime().try_into().unwrap();

        debug_assert!(metadata.st_uid() <= u16::MAX.into());
        let uid = (metadata.st_uid() & 0xFFFF) as u16;

        debug_assert!(metadata.st_gid() <= u16::MAX.into());
        let gid = (metadata.st_gid() & 0xFFFF) as u16;

        Self::Unix {
            atime,
            mtime,
            uid,
            gid,
        }
    }

    #[cfg(target_os = "windows")]
    fn new_windows(metadata: &Metadata) -> Self {
        use std::os::windows::fs::MetadataExt;

        let mtime = metadata.last_write_time();
        let atime = metadata.last_access_time();
        let ctime = metadata.creation_time();

        Self::Ntfs {
            mtime,
            atime,
            ctime,
        }
    }
}
