//! slot-bootlogo: prepare a boot logo for the TrimUI Brick.
//!
//! The bootloader shows /bootlogo.bmp from the internal boot partition. Rather than
//! guess what it accepts, the output copies the geometry of a reference BMP (the
//! device's original logo): same width, height, bit depth and row order. The user's
//! picture is fitted inside, centred, on black.
//!
//!   slot-bootlogo info FILE.bmp
//!   slot-bootlogo convert INPUT REFERENCE.bmp OUTPUT.bmp
use image::{imageops::FilterType, DynamicImage, ImageDecoder, ImageReader, RgbImage};
use std::{fs, io::Cursor, path::Path, process::ExitCode};

/// Never accept a reference larger than any panel this tool targets.
const MAX_SIDE: u32 = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Shape {
    width: u32,
    height: u32,
    bpp: u16,
    bottom_up: bool,
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}
fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Reads an uncompressed 24- or 32-bit BMP header.
fn shape(bytes: &[u8]) -> Result<Shape, String> {
    if bytes.get(0..2) != Some(b"BM") {
        return Err("reference is not a BMP".into());
    }
    let header = u32_at(bytes, 14).ok_or("truncated BMP")?;
    if header < 40 {
        return Err("unsupported BMP header".into());
    }
    let width = u32_at(bytes, 18).ok_or("truncated BMP")? as i32;
    let height = u32_at(bytes, 22).ok_or("truncated BMP")? as i32;
    let bpp = u16_at(bytes, 28).ok_or("truncated BMP")?;
    let compression = u32_at(bytes, 30).ok_or("truncated BMP")?;
    if !(bpp == 24 || bpp == 32) || !(compression == 0 || (bpp == 32 && compression == 3)) {
        return Err(format!("unsupported BMP: {bpp} bpp, compression {compression}"));
    }
    if width <= 0 || height == 0 || width as u32 > MAX_SIDE || height.unsigned_abs() > MAX_SIDE {
        return Err(format!("unsupported BMP size {width}x{height}"));
    }
    Ok(Shape { width: width as u32, height: height.unsigned_abs(), bpp, bottom_up: height > 0 })
}

fn decode(bytes: &[u8]) -> Result<DynamicImage, String> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    if reader.format().is_none() {
        return Err("not a PNG, JPEG or BMP image".into());
    }
    let mut decoder = reader.into_decoder().map_err(|e| format!("cannot read image: {e}"))?;
    // Phone photos are often stored sideways with an EXIF rotation tag.
    let orientation = decoder.orientation().ok();
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| format!("cannot read image: {e}"))?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    Ok(img)
}

/// Fit `img` inside `target`, centred on black. Transparency shows black.
fn compose(img: &DynamicImage, target: Shape) -> RgbImage {
    let (w, h) = (img.width().max(1), img.height().max(1));
    let scale = f64::min(target.width as f64 / w as f64, target.height as f64 / h as f64);
    let fw = ((w as f64 * scale).round() as u32).clamp(1, target.width);
    let fh = ((h as f64 * scale).round() as u32).clamp(1, target.height);
    // Big integer enlargements are usually pixel art: keep the pixels sharp.
    let filter = if scale >= 2.0 && (scale - scale.floor()) < 1e-9 {
        FilterType::Nearest
    } else {
        FilterType::Lanczos3
    };
    let fitted = if (fw, fh) == (w, h) { img.to_rgba8() } else { image::imageops::resize(&img.to_rgba8(), fw, fh, filter) };
    let mut out = RgbImage::new(target.width, target.height);
    let (ox, oy) = ((target.width - fw) / 2, (target.height - fh) / 2);
    for (x, y, p) in fitted.enumerate_pixels() {
        let a = p[3] as u32;
        let c = |v: u8| ((v as u32 * a + 127) / 255) as u8;
        out.put_pixel(ox + x, oy + y, image::Rgb([c(p[0]), c(p[1]), c(p[2])]));
    }
    out
}

/// Writes a BMP with exactly the reference's geometry.
fn encode(img: &RgbImage, s: Shape) -> Vec<u8> {
    let bytes_pp = (s.bpp / 8) as usize;
    let row = (s.width as usize * bytes_pp + 3) / 4 * 4;
    let data = row * s.height as usize;
    let mut out = Vec::with_capacity(54 + data);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(54 + data as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(s.width as i32).to_le_bytes());
    let h = s.height as i32;
    out.extend_from_slice(&(if s.bottom_up { h } else { -h }).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&s.bpp.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(data as u32).to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&2835u32.to_le_bytes());
    out.extend_from_slice(&[0; 8]);
    for i in 0..s.height {
        let y = if s.bottom_up { s.height - 1 - i } else { i };
        let start = out.len();
        for x in 0..s.width {
            let p = img.get_pixel(x, y);
            out.extend_from_slice(&[p[2], p[1], p[0]]);
            if bytes_pp == 4 {
                out.push(255);
            }
        }
        out.resize(start + row, 0);
    }
    out
}

fn convert(input: &Path, reference: &Path, output: &Path) -> Result<String, String> {
    let shape = shape(&fs::read(reference).map_err(|e| format!("{}: {e}", reference.display()))?)?;
    let picture = decode(&fs::read(input).map_err(|e| format!("{}: {e}", input.display()))?)?;
    let bmp = encode(&compose(&picture, shape), shape);
    fs::write(output, &bmp).map_err(|e| format!("{}: {e}", output.display()))?;
    Ok(format!(
        "{}x{} picture fitted into {}x{} {}-bit logo ({} bytes)",
        picture.width(), picture.height(), shape.width, shape.height, shape.bpp, bmp.len()
    ))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["--version"] => Ok("slot-bootlogo 1".into()),
        ["info", file] => fs::read(file)
            .map_err(|e| format!("{file}: {e}"))
            .and_then(|b| shape(&b))
            .map(|s| format!("{} {} {}", s.width, s.height, s.bpp)),
        ["convert", input, reference, output] => convert(Path::new(input), Path::new(reference), Path::new(output)),
        _ => Err("usage: slot-bootlogo info FILE.bmp | convert INPUT REFERENCE.bmp OUTPUT.bmp".into()),
    };
    match result {
        Ok(msg) => {
            println!("{msg}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("slot-bootlogo: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, Rgba, RgbaImage};

    fn bmp(s: Shape) -> Vec<u8> {
        encode(&RgbImage::new(s.width, s.height), s)
    }
    fn png(img: &RgbaImage) -> Vec<u8> {
        let mut v = Vec::new();
        img.write_to(&mut Cursor::new(&mut v), ImageFormat::Png).unwrap();
        v
    }

    #[test]
    fn header_roundtrip_and_padding() {
        let s = Shape { width: 197, height: 75, bpp: 24, bottom_up: true };
        let b = bmp(s);
        assert_eq!(shape(&b).unwrap(), s);
        assert_eq!(b.len(), 54 + 75 * 592); // 197*3=591 padded to 592
        let t = Shape { width: 10, height: 4, bpp: 32, bottom_up: false };
        assert_eq!(shape(&bmp(t)).unwrap(), t);
    }

    #[test]
    fn matches_the_shipped_slot_logo_geometry() {
        let b = fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../test-fixtures/slot-logo.bmp"));
        if let Ok(b) = b {
            let s = shape(&b).unwrap();
            assert_eq!((s.width, s.height, s.bpp), (196, 75, 24));
            assert_eq!(encode(&RgbImage::new(196, 75), s).len(), b.len());
        }
    }

    #[test]
    fn rejects_non_bmp_reference_and_garbage_input() {
        assert!(shape(b"PNG....").is_err());
        assert!(decode(b"definitely not an image").is_err());
    }

    #[test]
    fn wide_picture_is_letterboxed_and_centred() {
        let mut img = RgbaImage::from_pixel(40, 10, Rgba([255, 0, 0, 255]));
        img.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        let target = Shape { width: 20, height: 20, bpp: 24, bottom_up: true };
        let out = compose(&decode(&png(&img)).unwrap(), target);
        assert_eq!(out.get_pixel(10, 0).0, [0, 0, 0], "black bar above");
        assert_eq!(out.get_pixel(10, 10).0[0] > 200, true, "picture in the middle");
        assert_eq!(out.get_pixel(10, 19).0, [0, 0, 0], "black bar below");
    }

    #[test]
    fn transparency_becomes_black_and_pixel_art_stays_sharp() {
        let mut img = RgbaImage::from_pixel(2, 1, Rgba([255, 255, 255, 255]));
        img.put_pixel(1, 0, Rgba([255, 255, 255, 0]));
        let target = Shape { width: 8, height: 4, bpp: 24, bottom_up: true };
        let out = compose(&decode(&png(&img)).unwrap(), target);
        assert_eq!(out.get_pixel(0, 0).0, [255, 255, 255]);
        assert_eq!(out.get_pixel(3, 3).0, [255, 255, 255]);
        assert_eq!(out.get_pixel(4, 0).0, [0, 0, 0]);
    }

    #[test]
    fn bottom_up_rows_are_stored_last_row_first_in_bgr() {
        let mut img = RgbImage::new(1, 2);
        img.put_pixel(0, 0, image::Rgb([1, 2, 3]));
        img.put_pixel(0, 1, image::Rgb([4, 5, 6]));
        let b = encode(&img, Shape { width: 1, height: 2, bpp: 24, bottom_up: true });
        assert_eq!(&b[54..57], &[6, 5, 4]);
        assert_eq!(&b[58..61], &[3, 2, 1]);
    }

    #[test]
    fn convert_end_to_end() {
        let d = std::env::temp_dir().join(format!("slot-bootlogo-{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        let reference = d.join("ref.bmp");
        let s = Shape { width: 64, height: 48, bpp: 24, bottom_up: true };
        fs::write(&reference, bmp(s)).unwrap();
        let input = d.join("pic.png");
        fs::write(&input, png(&RgbaImage::from_pixel(300, 100, Rgba([0, 128, 255, 255])))).unwrap();
        let out = d.join("out.bmp");
        convert(&input, &reference, &out).unwrap();
        let b = fs::read(&out).unwrap();
        assert_eq!(shape(&b).unwrap(), s);
        assert_eq!(b.len(), bmp(s).len());
        fs::remove_dir_all(&d).unwrap();
    }
}
