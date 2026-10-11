//! JPEG metadata re-attachment (EXIF + ICC).
//!
//! Split out of the former `processor.rs` monolith. Best-effort by design:
//! the batch pipeline calls [`reattach_jpeg_metadata`] after encoding a JPEG
//! and simply keeps the plain bytes when anything fails.

use std::io::{Cursor, Read};
use std::path::Path;

use exif::experimental::Writer;
use exif::{Reader as ExifReader, Tag, Value};

/// Re-embed source JPEG metadata (EXIF + ICC) into a freshly encoded JPEG.
///
/// The `image` crate encoders drop every marker, so a re-encode otherwise
/// strips the camera/date/color profile that a photographer may rely on. We
/// re-attach them on a best-effort basis: any failure (no EXIF, undecodable
/// ICC, parse error) yields `None` and the caller keeps the encoded bytes, so
/// metadata preservation can never produce a broken file.
///
/// EXIF orientation is forced to 1 because the render step already bakes the
/// *real* orientation into the pixels; leaving the original tag would make a
/// viewer rotate the already-rotated image a second time.
pub(super) fn reattach_jpeg_metadata(source: &Path, out: &[u8]) -> Option<Vec<u8>> {
    if out.len() < 2 || out[0] != 0xFF || out[1] != 0xD8 {
        return None; // not a JPEG
    }
    let source_bytes = read_metadata_prefix(source)?;

    // EXIF (orientation normalized to 1) — only when we can both read and
    // rewrite it, so we never ship a tag that double-rotates the image.
    let exif_tiff = read_exif_tiff_with_normalized_orientation(&source_bytes);
    // ICC profile: re-embed the raw APP2 payload verbatim (keeps the "ICC"
    // identifier and chunk sequence the decoders expect).
    let icc_payload = find_jpeg_app2_icc(&source_bytes);

    let exif_app1 = exif_tiff.map(|tiff| build_app1_exif(&tiff));
    let icc_app2 = icc_payload.map(|p| build_app2_icc(&p));

    if exif_app1.is_none() && icc_app2.is_none() {
        return None;
    }

    let mut result = Vec::with_capacity(out.len() + 4096);
    result.extend_from_slice(&out[0..2]); // SOI
    if let Some(seg) = exif_app1 {
        result.extend_from_slice(&seg);
    }
    if let Some(seg) = icc_app2 {
        result.extend_from_slice(&seg);
    }
    result.extend_from_slice(&out[2..]); // remainder of the encoded stream
    Some(result)
}

/// At most this many bytes are read from the source when re-attaching JPEG
/// metadata. EXIF (APP1) and ICC (APP2) segments live before the first scan
/// (SOS) marker — near the start of the file — so re-attachment never needs
/// the whole (possibly tens-of-MB) source photo the decoder just streamed.
const METADATA_PREFIX_BYTES: u64 = 8 * 1024 * 1024;

/// Read at most [`METADATA_PREFIX_BYTES`] bytes from the start of `path`.
/// A segment that (absurdly) extends past the cap fails to parse downstream
/// and is then simply omitted — re-attachment stays best-effort either way.
pub(super) fn read_metadata_prefix(path: &Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    let cap = file.metadata().ok()?.len().min(METADATA_PREFIX_BYTES);
    let mut buf = Vec::with_capacity(cap as usize);
    file.take(cap).read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// Read EXIF from a JPEG and return the TIFF blob with Orientation forced to 1.
///
/// Returns `None` when there is no EXIF at all, or when the blob cannot be
/// parsed/re-serialized (so the caller simply omits the EXIF segment rather
/// than risking a malformed one).
pub(super) fn read_exif_tiff_with_normalized_orientation(jpeg: &[u8]) -> Option<Vec<u8>> {
    let reader = ExifReader::new();
    let exif = reader.read_from_container(&mut Cursor::new(jpeg)).ok()?;

    // Rebuild owned fields, forcing Orientation to 1 (normal). The pipeline has
    // already baked the real rotation into the pixels, so keeping the original
    // tag would double-rotate the image in an EXIF-aware viewer. `Field` is a
    // public struct literal (tag / ifd_num / value), so we clone every value
    // and swap only the Orientation one.
    let fields: Vec<exif::Field> = exif
        .fields()
        .map(|f| {
            if f.tag == Tag::Orientation {
                exif::Field {
                    tag: Tag::Orientation,
                    ifd_num: f.ifd_num,
                    value: Value::Short(vec![1u16]),
                }
            } else {
                exif::Field {
                    tag: f.tag,
                    ifd_num: f.ifd_num,
                    value: f.value.clone(),
                }
            }
        })
        .collect();
    if fields.is_empty() {
        return None;
    }

    let mut writer = Writer::new();
    for f in &fields {
        writer.push_field(f);
    }
    let mut cursor = Cursor::new(Vec::new());
    writer.write(&mut cursor, exif.little_endian()).ok()?;
    Some(cursor.into_inner())
}

/// Locate the ICC color-profile APP2 payload inside a JPEG. The returned slice
/// is the segment data (after the 2-byte length), still carrying the leading
/// `"ICC"` identifier and chunk sequence byte the decoders expect.
pub(super) fn find_jpeg_app2_icc(jpeg: &[u8]) -> Option<Vec<u8>> {
    if !(jpeg.len() >= 2 && jpeg[0] == 0xFF && jpeg[1] == 0xD8) {
        return None;
    }
    let mut i = 2usize;
    while i + 4 < jpeg.len() {
        if jpeg[i] != 0xFF {
            break;
        }
        let marker = jpeg[i + 1];
        i += 2;
        // Standalone markers carry no length payload.
        if marker == 0xD9 || marker == 0xDA || (0xD0..=0xD7).contains(&marker) {
            if marker == 0xDA {
                break; // SOS: compressed image data follows, stop scanning.
            }
            continue;
        }
        if i + 2 > jpeg.len() {
            break;
        }
        let len = u16::from_be_bytes([jpeg[i], jpeg[i + 1]]) as usize;
        if len < 2 || i + len > jpeg.len() {
            break;
        }
        let data = &jpeg[i + 2..i + len];
        if marker == 0xE2 && data.len() >= 4 && &data[0..4] == b"ICC" {
            return Some(data.to_vec());
        }
        i += len;
    }
    None
}

/// Wrap an EXIF TIFF blob in an APP1 segment: `FF E1 <len> "Exif\0\0" <tiff>`.
pub(super) fn build_app1_exif(tiff: &[u8]) -> Vec<u8> {
    let mut seg = Vec::with_capacity(8 + tiff.len());
    seg.push(0xFF);
    seg.push(0xE1);
    let len = (2 + 6 + tiff.len()) as u16; // length field counts itself + payload
    seg.extend_from_slice(&len.to_be_bytes());
    seg.extend_from_slice(b"Exif\0\0");
    seg.extend_from_slice(tiff);
    seg
}

/// Wrap an ICC profile payload in an APP2 segment: `FF E2 <len> <payload>`.
pub(super) fn build_app2_icc(payload: &[u8]) -> Vec<u8> {
    let mut seg = Vec::with_capacity(4 + payload.len());
    seg.push(0xFF);
    seg.push(0xE2);
    let len = (2 + payload.len()) as u16;
    seg.extend_from_slice(&len.to_be_bytes());
    seg.extend_from_slice(payload);
    seg
}
