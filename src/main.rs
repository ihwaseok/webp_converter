// 배포 시 까만 터미널 창(콘솔)이 뜨지 않도록 설정
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod converter;

use eframe::egui;
use rfd::FileDialog;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use walkdir::WalkDir;
use rayon::prelude::*;

#[derive(Default)]
struct ConverterApp {
    source_dir: Option<PathBuf>,
    image_files: Vec<PathBuf>,
    is_converting: bool,
    completed_count: Arc<AtomicUsize>,
    total_to_convert: usize,
    total_original_size: u64,
    status_msg: String,
    logs: Arc<Mutex<Vec<String>>>,
}

impl ConverterApp {
    fn format_size(bytes: u64) -> String {
        if bytes < 1024 {
            format!("{} B", bytes)
        } else if bytes < 1024 * 1024 {
            format!("{:.2} KB", bytes as f64 / 1024.0)
        } else if bytes < 1024 * 1024 * 1024 {
            format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
        } else {
            format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
        }
    }

    fn scan_directory(&mut self) {
        if let Some(ref path) = self.source_dir {
            self.image_files.clear();
            self.total_original_size = 0;
            self.logs.lock().unwrap().clear();
            
            for entry in WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
                if entry.file_type().is_file() {
                    if let Some(kind) = converter::detect_image_type(entry.path()) {
                        let metadata = std::fs::metadata(entry.path()).ok();
                        let size = metadata.map(|m| m.len()).unwrap_or(0);
                        
                        self.image_files.push(entry.path().to_path_buf());
                        self.total_original_size += size;
                        
                        let relative_path = entry.path().strip_prefix(path).unwrap_or(entry.path());
                        self.add_log(format!("발견({}): {} ({})", kind, relative_path.to_string_lossy(), Self::format_size(size)));
                    }
                }
            }
            
            let original_str = Self::format_size(self.total_original_size);
            let expected_size = (self.total_original_size as f64 * 0.6) as u64;
            let expected_str = Self::format_size(expected_size);
            
            self.status_msg = format!(
                "총 {}개 발견 | 현재 용량: {} | 예상 용량: {} (약 40% 절감)",
                self.image_files.len(), original_str, expected_str
            );
        }
    }

    fn add_log(&self, msg: String) {
        self.logs.lock().unwrap().push(msg);
    }

    fn start_conversion(&mut self) {
        if self.image_files.is_empty() {
            self.status_msg = "변환할 이미지가 없습니다!".to_string();
            return;
        }

        self.is_converting = true;
        self.total_to_convert = self.image_files.len();
        self.completed_count.store(0, Ordering::SeqCst);
        
        let files = self.image_files.clone();
        let logs = Arc::clone(&self.logs);
        let counter = Arc::clone(&self.completed_count);
        
        std::thread::spawn(move || {
            files.par_iter().for_each(|path| {
                match converter::convert_to_webp(path) {
                    Ok(out) => {
                        logs.lock().unwrap().push(format!("대체 완료: {}", out.file_name().unwrap().to_string_lossy()));
                    }
                    Err(e) => {
                        logs.lock().unwrap().push(format!("오류 발생({}): {}", path.file_name().unwrap().to_string_lossy(), e));
                    }
                }
                counter.fetch_add(1, Ordering::SeqCst);
            });
        });
        
        self.status_msg = "하위 폴더 포함 전체 변환 진행 중...".to_string();
    }
}

impl eframe::App for ConverterApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let completed = self.completed_count.load(Ordering::SeqCst);
        
        if self.is_converting {
            if completed >= self.total_to_convert && self.total_to_convert > 0 {
                self.is_converting = false;
                self.status_msg = format!("완료! 총 {}개의 파일이 WebP로 교체되었습니다.", completed);
            } else {
                self.status_msg = format!("변환 중... ({}/{})", completed, self.total_to_convert);
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("🚀 Rust 일괄 WebP 변환기 (단일 포터블 버전)");
            ui.add_space(10.0);

            ui.horizontal(|ui| {
                if ui.button("📁 폴더 선택").clicked() {
                    if let Some(path) = FileDialog::new().pick_folder() {
                        self.source_dir = Some(path);
                        self.scan_directory();
                    }
                }
                
                if let Some(ref path) = self.source_dir {
                    ui.label(format!("선택된 경로: {}", path.display()));
                }
            });

            ui.add_space(10.0);

            if ui.button("⚡ 하위 폴더까지 모두 변환 (원본 삭제)").clicked() && !self.is_converting {
                self.start_conversion();
            }

            ui.add_space(10.0);
            
            let msg_color = if self.is_converting { egui::Color32::GOLD } else { egui::Color32::LIGHT_BLUE };
            ui.colored_label(msg_color, &self.status_msg);
            
            if self.is_converting {
                let progress = completed as f32 / self.total_to_convert as f32;
                ui.add(egui::ProgressBar::new(progress).show_percentage());
            }

            ui.add_space(10.0);
            ui.separator();
            ui.heading("로그");
            
            egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                let logs = self.logs.lock().unwrap();
                for log in logs.iter() {
                    ui.label(log);
                }
            });
        });
        
        if self.is_converting {
            ctx.request_repaint();
        }
    }
}

// 폰트를 바이너리에 내장시키는 함수
fn setup_custom_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // 프로젝트 폴더의 "font.ttf" 파일을 읽어 바이너리에 포함시킵니다.
    // 만약 파일명을 다르게 하고 싶다면 이 부분의 이름을 수정해 주세요.
    // (예: C:\Windows\Fonts\malgun.ttf 를 직접 지정하거나 프로젝트 폴더로 복사해 오세요)
    let font_bytes = include_bytes!("C:\\Windows\\Fonts\\malgun.ttf");

    fonts.font_data.insert(
        "embedded_font".to_owned(),
        egui::FontData::from_static(font_bytes),
    );

    fonts.families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .insert(0, "embedded_font".to_owned());

    fonts.families
        .get_mut(&egui::FontFamily::Monospace)
        .unwrap()
        .push("embedded_font".to_owned());

    ctx.set_fonts(fonts);
}

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([650.0, 500.0]),
        ..Default::default()
    };
    
    eframe::run_native(
        "이미지 WebP 변환기",
        options,
        Box::new(|cc| {
            setup_custom_fonts(&cc.egui_ctx);
            Box::new(ConverterApp::default())
        }),
    )
}
