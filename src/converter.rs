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

    // WebP magic bytes: "RIFF....WEBP" — 정적/애니메이션 모두 reencode_webp에서 처리
    let is_webp = bytes.len() >= 12
        && bytes.starts_with(b"RIFF")
        && &bytes[8..12] == b"WEBP";

    if is_webp {
        return reencode_webp(source, &bytes);
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

fn calculate_new_dimensions(w: u32, h: u32) -> (u32, u32) {
    if w > 800 {
        let ratio = 800.0 / w as f64;
        let new_h = (h as f64 * ratio).round() as u32;
        (800, new_h)
    } else {
        (w, h)
    }
}

fn reencode_webp(source: &Path, bytes: &[u8]) -> Result<PathBuf> {
    let decoder = webp::AnimDecoder::new(bytes);
    let decoded = decoder.decode()
        .map_err(|e| anyhow!("WebP 디코딩 실패: {}", e))?;

    if decoded.has_animation() {
        // 애니메이션 WebP → 프레임 타임스탬프를 그대로 유지하며 재인코딩
        let first = decoded.get_frame(0)
            .ok_or_else(|| anyhow!("WebP 프레임 없음"))?;

        let (n_width, n_height) = calculate_new_dimensions(first.width(), first.height());

        let mut config = webp::WebPConfig::new()
            .map_err(|_| anyhow!("WebPConfig 생성 실패"))?;
        config.quality = 75.0;
        config.method = 4; // 인코딩 방식 최적화

        let mut enc = webp::AnimEncoder::new(n_width, n_height, &config);

        // AnimFrame이 픽셀 데이터를 참조로 저장하므로, 데이터를 먼저 수집해 수명을 보장
        let frames_data: Vec<(Vec<u8>, u32, u32, i32)> = (0..decoded.len())
            .filter_map(|i| decoded.get_frame(i))
            .map(|f| {
                let (fw, fh) = (f.width(), f.height());
                if fw > 800 {
                    let img = image::RgbaImage::from_raw(fw, fh, f.get_image().to_vec()).unwrap();
                    let resized = image::imageops::resize(&img, n_width, n_height, image::imageops::FilterType::Lanczos3);
                    (resized.into_raw(), n_width, n_height, f.get_time_ms() as i32)
                } else {
                    (f.get_image().to_vec(), fw, fh, f.get_time_ms() as i32)
                }
            })
            .collect();

        for (rgba, w, h, ts) in &frames_data {
            enc.add_frame(webp::AnimFrame::from_rgba(rgba, *w, *h, *ts));
        }

        let webp_data = enc.try_encode()
            .map_err(|e| anyhow!("애니메이션 WebP 인코딩 실패: {:?}", e))?;
        fs::write(source, &*webp_data)?;
    } else {
        // 정적 WebP → quality 75로 재인코딩
        let img = image::load_from_memory_with_format(bytes, image::ImageFormat::WebP)?;
        let encoder = Encoder::from_image(&img)
            .map_err(|e| anyhow!("WebP 인코더 생성 실패: {}", e))?;
        let webp_data = encoder.encode(75.0);
        fs::write(source, &*webp_data)?;
    }

    Ok(source.to_path_buf())
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
    let (n_width, n_height) = calculate_new_dimensions(width, height);

    let mut config = webp::WebPConfig::new()
        .map_err(|_| anyhow!("WebPConfig 생성 실패"))?;
    config.quality = 75.0;
    config.method = 4; // 인코딩 방식 최적화

    let mut enc = webp::AnimEncoder::new(n_width, n_height, &config);

    let mut timestamp_ms: i32 = 0;
    let frames_data: Vec<(Vec<u8>, u32, u32, i32)> = frames.iter().map(|frame| {
        let rgba = frame.buffer();
        let (fw, fh) = rgba.dimensions();

        let (num, den) = frame.delay().numer_denom_ms();
        let delay_ms = ((num as f64 / den as f64).round() as i32).max(20);
        let current_ts = timestamp_ms;
        timestamp_ms += delay_ms;

        if width > 800 {
            let resized = image::imageops::resize(rgba, n_width, n_height, image::imageops::FilterType::Lanczos3);
            (resized.into_raw(), n_width, n_height, current_ts)
        } else {
            (rgba.as_raw().to_vec(), fw, fh, current_ts)
        }
    }).collect();

    for (rgba, w, h, ts) in &frames_data {
        enc.add_frame(webp::AnimFrame::from_rgba(rgba, *w, *h, *ts));
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
