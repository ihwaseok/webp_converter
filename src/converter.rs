use anyhow::{anyhow, Result};
use image::GenericImageView;
use std::fs;
use std::path::{Path, PathBuf};
use webp::Encoder;

pub fn detect_image_type(path: &Path) -> Option<&'static str> {
    // magic byte 판별에는 첫 12바이트면 충분 — 파일 전체를 읽을 필요 없음
    use std::io::Read;
    let mut f = fs::File::open(path).ok()?;
    let mut bytes = [0u8; 12];
    let n = f.read(&mut bytes).ok()?;
    let kind = infer::get(&bytes[..n])?;
    match kind.mime_type() {
        "image/jpeg" => Some("jpeg"),
        "image/png" => Some("png"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        "image/bmp" => Some("bmp"),
        "image/tiff" => Some("tiff"),
        _ => None,
    }
}

pub fn convert_to_webp(source: &Path) -> Result<PathBuf> {
    let bytes = fs::read(source)?;

    // GIF magic bytes: "GIF8" — 애니메이션 여부를 확인해야 하므로 별도 처리
    if bytes.starts_with(b"GIF8") {
        return convert_gif_to_webp(source, &bytes);
    }

    let img = image::load_from_memory(&bytes)?;
    let (_width, _height) = img.dimensions();
    let encoder = Encoder::from_image(&img)
        .map_err(|e| anyhow!("WebP 인코더 생성 실패: {}", e))?;
    let webp_data = encoder.encode(75.0);

    let target_path = source.with_extension("webp");
    fs::write(&target_path, &*webp_data)?;
    if source != target_path {
        fs::remove_file(source)?;
    }
    Ok(target_path)
}

fn convert_gif_to_webp(source: &Path, bytes: &[u8]) -> Result<PathBuf> {
    use image::AnimationDecoder;
    use image::codecs::gif::GifDecoder;
    use std::io::Cursor;

    let decoder = GifDecoder::new(Cursor::new(bytes))?;
    let frames: Vec<image::Frame> = decoder.into_frames().collect_frames()?;

    // 단일 프레임 GIF — 정적 WebP로 변환
    if frames.len() <= 1 {
        let img = image::load_from_memory(bytes)?;
        let encoder = Encoder::from_image(&img)
            .map_err(|e| anyhow!("WebP 인코더 생성 실패: {}", e))?;
        let webp_data = encoder.encode(75.0);
        let target_path = source.with_extension("webp");
        fs::write(&target_path, &*webp_data)?;
        if source != target_path {
            fs::remove_file(source)?;
        }
        return Ok(target_path);
    }

    // 애니메이션 GIF → 애니메이션 WebP
    let (width, height) = frames[0].buffer().dimensions();

    // AnimEncoder::new(w, h, &config) — AnimEncoderOptions 없음
    let config = webp::WebPConfig::new()
        .map_err(|_| anyhow!("WebPConfig 생성 실패"))?;
    let mut enc = webp::AnimEncoder::new(width, height, &config);

    let mut timestamp_ms: i32 = 0;

    for frame in &frames {
        let rgba = frame.buffer();
        let (fw, fh) = rgba.dimensions();

        // image::Delay::numer_denom_ms() → delay = numer/denom (ms 단위)
        let (num, den) = frame.delay().numer_denom_ms();
        let delay_ms = ((num as f64 / den as f64).round() as i32).max(20);

        // from_rgba: Result 아닌 AnimFrame 직접 반환
        let anim_frame = webp::AnimFrame::from_rgba(rgba.as_raw(), fw, fh, timestamp_ms);
        // add_frame: () 반환
        enc.add_frame(anim_frame);

        timestamp_ms += delay_ms;
    }

    let webp_data = enc.try_encode()
        .map_err(|e| anyhow!("애니메이션 WebP 인코딩 실패: {:?}", e))?;

    let target_path = source.with_extension("webp");
    fs::write(&target_path, &*webp_data)?;
    if source != target_path {
        fs::remove_file(source)?;
    }
    Ok(target_path)
}
