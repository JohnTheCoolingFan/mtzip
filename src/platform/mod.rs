//! Platform-specific stuff

use std::fs::Metadata;

use cfg_if::cfg_if;

// VERSION_MADE_BY
cfg_if! {
    if #[cfg(target_os = "windows")] {
        /// OS - Windows, id 11 per Info-Zip spec
        /// Specification version 6.2
        pub(crate) const VERSION_MADE_BY: u16 = (11 << 8) + 62;
    } else if #[cfg(target_os = "macos")] {
        /// OS - MacOS darwin, id 19
        /// Specification version 6.2
        pub(crate) const VERSION_MADE_BY: u16 = (19 << 8) + 62;
    } else {
        // Fallback
        /// OS - Unix assumed, id 3
        /// Specification version 6.2
        pub(crate) const VERSION_MADE_BY: u16 = (3 << 8) + 62;
    }
}

#[allow(dead_code)]
pub(crate) const DEFAULT_UNIX_FILE_ATTRS: u16 = 0o100644;
#[allow(dead_code)]
pub(crate) const DEFAULT_UNIX_DIR_ATTRS: u16 = 0o040755;

#[cfg(target_os = "windows")]
pub(crate) const DEFAULT_WINDOWS_FILE_ATTRS: u16 = 128;
#[cfg(target_os = "windows")]
pub(crate) const DEFAULT_WINDOWS_DIR_ATTRS: u16 = 16;

#[inline]
#[allow(dead_code)]
const fn convert_attrs(attrs: u32) -> u16 {
    attrs as u16
}

#[inline]
pub(crate) fn attributes_from_fs(metadata: &Metadata) -> u16 {
    cfg_if! {
        if #[cfg(target_os = "windows")] {
            use std::os::windows::fs::MetadataExt;
            convert_attrs(metadata.file_attributes())
        } else if #[cfg(target_os = "linux")] {
            use std::os::linux::fs::MetadataExt;
            convert_attrs(metadata.st_mode())
        } else if #[cfg(target_os = "macos")] {
            use std::os::darwin::fs::MetadataExt;
            convert_attrs(metadata.st_mode())
        } else if #[cfg(all(unix, not(target_os = "linux"), not(target_os = "macos")))] {
            use std::os::unix::fs::PermissionsExt;
            convert_attrs(metadata.permissions().mode())
        } else {
            if metadata.is_dir() {
                DEFAULT_UNIX_DIR_ATTRS
            } else {
                DEFAULT_UNIX_FILE_ATTRS
            }
        }
    }
}

pub(crate) const fn default_file_attrs() -> u16 {
    cfg_if! {
        if #[cfg(target_os = "windows")] {
            DEFAULT_WINDOWS_FILE_ATTRS
        } else if #[cfg(any(target_os = "linux", unix))] {
            DEFAULT_UNIX_FILE_ATTRS
        } else {
            0
        }
    }
}

pub(crate) const fn default_dir_attrs() -> u16 {
    cfg_if! {
        if #[cfg(target_os = "windows")] {
            DEFAULT_WINDOWS_DIR_ATTRS
        } else if #[cfg(any(target_os = "linux", unix))] {
            DEFAULT_UNIX_DIR_ATTRS
        } else {
            0
        }
    }
}
