//! `esp_app_desc_t` parsing.
//!
//! Every ESP-IDF application image stores a 256-byte descriptor at offset
//! `0x20`; the bootloader uses it for version/secure-version checks, and the OTA
//! client uses it to report what is running. The layout is fixed:
//!
//! | offset | field |
//! |---|---|
//! | 0x00 | magic `0xABCD5432`, secure version |
//! | 0x10 | version (32) |
//! | 0x30 | project name (32) |
//! | 0x50 | build time (16) |
//! | 0x60 | build date (16) |
//! | 0x70 | IDF version (32) |
//! | 0x90 | ELF SHA-256 (32) |
//!
//! Strings are NUL-padded, not necessarily NUL-terminated.

/// Offset of the descriptor inside the application image.
pub const APP_DESC_OFFSET: usize = 0x20;

/// `ESP_APP_DESC_MAGIC_WORD`.
pub const APP_DESC_MAGIC: u32 = 0xABCD_5432;

/// Size of the descriptor.
pub const APP_DESC_SIZE: usize = 196;

/// Fields of interest from an app descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppDescriptor<'a> {
    /// Application version string.
    pub version: &'a str,
    /// Project name.
    pub project: &'a str,
    /// Build time.
    pub built_at: &'a str,
    /// Build date.
    pub built_on: &'a str,
    /// ESP-IDF version used to build the image.
    pub idf_version: &'a str,
}

/// Why an image descriptor was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageError {
    /// The image is shorter than the descriptor.
    TooShort,
    /// The magic word does not match.
    BadMagic,
    /// A string field is not valid UTF-8.
    NotUtf8,
}

/// Parse the app descriptor from the start of an application image.
///
/// # Errors
/// [`ImageError::TooShort`] if the image cannot contain a descriptor,
/// [`ImageError::BadMagic`] if it is not an ESP-IDF application image,
/// [`ImageError::NotUtf8`] if a string field is malformed.
pub fn parse_app_descriptor(image: &[u8]) -> Result<AppDescriptor<'_>, ImageError> {
    let base = APP_DESC_OFFSET;
    if image.len() < base + APP_DESC_SIZE {
        return Err(ImageError::TooShort);
    }
    let magic = u32::from_le_bytes([
        image[base],
        image[base + 1],
        image[base + 2],
        image[base + 3],
    ]);
    if magic != APP_DESC_MAGIC {
        return Err(ImageError::BadMagic);
    }
    Ok(AppDescriptor {
        version: field(&image[base + 0x10..base + 0x30])?,
        project: field(&image[base + 0x30..base + 0x50])?,
        built_at: field(&image[base + 0x50..base + 0x60])?,
        built_on: field(&image[base + 0x60..base + 0x70])?,
        idf_version: field(&image[base + 0x70..base + 0x90])?,
    })
}

fn field(bytes: &[u8]) -> Result<&str, ImageError> {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    core::str::from_utf8(&bytes[..end]).map_err(|_| ImageError::NotUtf8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(image: &mut [u8], offset: usize, text: &str) {
        image[APP_DESC_OFFSET + offset..APP_DESC_OFFSET + offset + text.len()]
            .copy_from_slice(text.as_bytes());
    }

    fn image() -> [u8; APP_DESC_OFFSET + APP_DESC_SIZE] {
        let mut image = [0u8; APP_DESC_OFFSET + APP_DESC_SIZE];
        image[APP_DESC_OFFSET..APP_DESC_OFFSET + 4].copy_from_slice(&APP_DESC_MAGIC.to_le_bytes());
        put(&mut image, 0x10, "1.2.3");
        put(&mut image, 0x30, "sensor-node");
        put(&mut image, 0x50, "12:00:00");
        put(&mut image, 0x60, "2026-09-21");
        put(&mut image, 0x70, "v5.5.1");
        image
    }

    #[test]
    fn parses_a_valid_descriptor() {
        let image = image();
        let parsed = parse_app_descriptor(&image).unwrap();
        assert_eq!(parsed.version, "1.2.3");
        assert_eq!(parsed.project, "sensor-node");
        assert_eq!(parsed.built_at, "12:00:00");
        assert_eq!(parsed.built_on, "2026-09-21");
        assert_eq!(parsed.idf_version, "v5.5.1");
    }

    #[test]
    fn rejects_short_images() {
        let image = [0u8; APP_DESC_OFFSET + APP_DESC_SIZE - 1];
        assert_eq!(parse_app_descriptor(&image), Err(ImageError::TooShort));
    }

    #[test]
    fn rejects_a_missing_magic() {
        let mut image = image();
        image[APP_DESC_OFFSET] ^= 0xFF;
        assert_eq!(parse_app_descriptor(&image), Err(ImageError::BadMagic));
    }

    #[test]
    fn rejects_non_utf8_fields() {
        let mut image = image();
        image[APP_DESC_OFFSET + 0x10] = 0xFF;
        assert_eq!(parse_app_descriptor(&image), Err(ImageError::NotUtf8));
    }

    #[test]
    fn accepts_nul_padded_fields_without_terminator() {
        let mut image = image();
        // Fill the whole version field with 'x', no NUL at all.
        for byte in &mut image[APP_DESC_OFFSET + 0x10..APP_DESC_OFFSET + 0x30] {
            *byte = b'x';
        }
        let expected = "x".repeat(32);
        assert_eq!(
            parse_app_descriptor(&image).unwrap().version,
            expected.as_str()
        );
    }
}
