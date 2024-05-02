pub mod data;
pub mod extra_field;
pub mod file;
pub mod job;
use std::io::Seek;
#[inline]
pub fn stream_position_u32<W: Seek>(buf: &mut W) -> std::io::Result<u32> {
    let offset = buf.stream_position()?;
    debug_assert!(offset <= u32::MAX.into());
    Ok(offset as u32)
}
