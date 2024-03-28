//! ZIP file extra field

use std::{fs::Metadata, io::Write};

use cfg_if::cfg_if;

/// This is a structure containing [`ExtraField`]s associated with a file or directory in a zip
/// file, mostly used for filesystem properties, and this is the only functionality implemented
/// here.
///
/// The [`new_from_fs`](Self::new_from_fs) method will use the metadata the filesystem provides to
/// construct the collection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ExtraFields {
    pub(crate) values: Vec<ExtraField>,
}

impl ExtraFields {
    pub(crate) fn data_length(&self, central_header: bool) -> u16 {
        self.values
            .iter()
            .map(|f| 4 + f.field_size(central_header))
            .sum()
    }

    pub fn new_from_fs(metadata: &Metadata) -> Self {
        cfg_if! {
            if #[cfg(target_os = "windows")] {
                Self::new_windows(metadata)
            } else if #[cfg(target_os = "linux")] {
                Self::new_linux(metadata)
            } else if #[cfg(unix)] {
                Self::new_unix(metadata)
            } else {
                Self::default()
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn new_linux(metadata: &Metadata) -> Self {
        use std::os::linux::fs::MetadataExt;

        let mod_time = Some(metadata.st_mtime() as i32);
        let ac_time = Some(metadata.st_atime() as i32);
        let cr_time = Some(metadata.st_ctime() as i32);

        let uid = metadata.st_uid();
        let gid = metadata.st_gid();

        Self {
            values: vec![
                ExtraField::UnixExtendedTimestamp {
                    mod_time,
                    ac_time,
                    cr_time,
                },
                ExtraField::UnixAttrs { uid, gid },
            ],
        }
    }

    #[cfg(unix)]
    #[allow(dead_code)]
    fn new_unix(metadata: &Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;

        let mod_time = Some(metadata.mtime() as i32);
        let ac_time = Some(metadata.atime() as i32);
        let cr_time = Some(metadata.ctime() as i32);

        let uid = metadata.uid();
        let gid = metadata.gid();

        Self {
            values: vec![
                ExtraField::UnixExtendedTimestamp {
                    mod_time,
                    ac_time,
                    cr_time,
                },
                ExtraField::UnixAttrs { uid, gid },
            ],
        }
    }

    #[cfg(target_os = "windows")]
    fn new_windows(metadata: &Metadata) -> Self {
        use std::os::windows::fs::MetadataExt;

        let mtime = metadata.last_write_time();
        let atime = metadata.last_access_time();
        let ctime = metadata.creation_time();

        Self {
            values: vec![ExtraField::Ntfs {
                mtime,
                atime,
                ctime,
            }],
        }
    }

    pub(crate) fn write<W: Write>(
        &self,
        writer: &mut W,
        central_header: bool,
    ) -> std::io::Result<()> {
        for field in &self.values {
            field.write(writer, central_header)?;
        }
        Ok(())
    }

    pub(crate) fn new<I>(fields: I) -> Self
    where
        I: IntoIterator<Item = ExtraField>,
    {
        Self {
            values: fields.into_iter().collect(),
        }
    }
}

/// Extra data that can be associated with a file or directory.
///
/// This library only implements the filesystem properties in NTFS or UNIX format.
///
/// The [`new_from_fs`](Self::new_from_fs) method will use the metadata the filesystem provides to
/// construct the collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtraField {
    /// NTFS file properties.
    Ntfs {
        /// Last modification timestamp
        mtime: u64,
        /// Last access timestamp
        atime: u64,
        /// File/directory creation timestamp
        ctime: u64,
    },
    UnixExtendedTimestamp {
        mod_time: Option<i32>,
        ac_time: Option<i32>,
        cr_time: Option<i32>,
    },
    UnixAttrs {
        uid: u32,
        gid: u32,
    },
}

const MOD_TIME_PRESENT: u8 = 1;
const AC_TIME_PRESENT: u8 = 1 << 1;
const CR_TIME_PRESENT: u8 = 1 << 2;

impl ExtraField {
    #[inline]
    fn header_id(&self) -> u16 {
        match self {
            Self::Ntfs {
                mtime: _,
                atime: _,
                ctime: _,
            } => 0x000a,
            Self::UnixExtendedTimestamp {
                mod_time: _,
                ac_time: _,
                cr_time: _,
            } => 0x5455,
            Self::UnixAttrs { uid: _, gid: _ } => 0x7875,
        }
    }

    #[inline]
    const fn optional_field_size<T: Sized>(field: &Option<T>) -> u16 {
        match field {
            Some(_) => std::mem::size_of::<T>() as u16,
            None => 0,
        }
    }

    #[inline]
    fn field_size(&self, central_header: bool) -> u16 {
        match self {
            Self::Ntfs {
                mtime: _,
                atime: _,
                ctime: _,
            } => 32,
            Self::UnixExtendedTimestamp {
                mod_time,
                ac_time,
                cr_time,
            } => {
                1 + Self::optional_field_size(mod_time)
                    + (!central_header)
                        .then(|| {
                            Self::optional_field_size(ac_time) + Self::optional_field_size(cr_time)
                        })
                        .unwrap_or(0)
            }
            Self::UnixAttrs { uid: _, gid: _ } => 11,
        }
    }

    #[inline]
    const fn if_present(val: Option<i32>, if_present: u8) -> u8 {
        match val {
            Some(_) => if_present,
            None => 0,
        }
    }

    pub(crate) fn write<W: Write>(
        self,
        writer: &mut W,
        central_header: bool,
    ) -> std::io::Result<()> {
        // Header ID
        writer.write_all(&self.header_id().to_le_bytes())?;
        // Field data size
        writer.write_all(&self.field_size(central_header).to_le_bytes())?;

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
            Self::UnixExtendedTimestamp {
                mod_time,
                ac_time,
                cr_time,
            } => {
                let flags = Self::if_present(mod_time, MOD_TIME_PRESENT)
                    | Self::if_present(ac_time, AC_TIME_PRESENT)
                    | Self::if_present(cr_time, CR_TIME_PRESENT);
                writer.write_all(&[flags])?;
                if let Some(mod_time) = mod_time {
                    writer.write_all(&mod_time.to_le_bytes())?;
                }
                if !central_header {
                    if let Some(ac_time) = ac_time {
                        writer.write_all(&ac_time.to_le_bytes())?;
                    }
                    if let Some(cr_time) = cr_time {
                        writer.write_all(&cr_time.to_le_bytes())?;
                    }
                }
            }
            Self::UnixAttrs { uid, gid } => {
                // Version of the field
                writer.write_all(&[1])?;
                // UID size
                writer.write_all(&[4])?;
                // UID
                writer.write_all(&uid.to_le_bytes())?;
                // GID size
                writer.write_all(&[4])?;
                // GID
                writer.write_all(&gid.to_le_bytes())?;
            }
        }

        Ok(())
    }
}
