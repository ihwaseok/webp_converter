use anyhow::{anyhow, Result};
use image::GenericImageView;
use std::fs;
use std::path::{Path, PathBuf};
use webp::Encoder;

pub fn detect_image_type(path: &Path) -> Option<&'static str> {
    let bytes = fs::read(path).ok()?;
    let kind = infer::get(&bytes)?;
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
    let img = image::load_from_memory(&bytes)?;
    
    let (_width, _height) = img.dimensions();
    let encoder = Encoder::from_image(&img)
        .map_err(|e| anyhow!("WebP 인코더 생성 실패: {}", e))?;
    
    let webp_data = encoder.encode(75.0);
    
    // 원본과 같은 위치에 확장자만 .webp로 변경
    let target_path = source.with_extension("webp");
    
    // 1. 새로운 WebP 파일 저장
    fs::write(&target_path, &*webp_data)?;
    
    // 2. 만약 원본과 대상 경로가 다르다면(즉, 원본이 이미 .webp가 아니라면) 원본 삭제
    if source != target_path {
        fs::remove_file(source)?;
    }
    
    Ok(target_path)
}
