use std::sync::Arc;

use crate::stage::{StageCatalog, StageRecognition};

pub struct OcrImage {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}

pub fn crop_title(
    data: &[u8],
    width: u32,
    height: u32,
    row_pitch: u32,
) -> Result<OcrImage, String> {
    if width < 640
        || height < 360
        || row_pitch < width.saturating_mul(4)
        || data.len() < row_pitch as usize * height as usize
    {
        return Err("OCR 帧缓冲区尺寸无效".to_string());
    }
    let scale = (width as f64 / 1920.0).min(height as f64 / 1080.0);
    let offset_x = (width as f64 - 1920.0 * scale) / 2.0;
    let offset_y = (height as f64 - 1080.0 * scale) / 2.0;
    let reference =
        |value: u32, offset: f64| (offset + value as f64 * scale).round().max(0.0) as u32;
    let left = reference(420, offset_x).min(width);
    let right = reference(1500, offset_x).min(width);
    let top = reference(180, offset_y).min(height);
    let bottom = reference(500, offset_y).min(height);
    if left >= right || top >= bottom {
        return Err("OCR 标题区域无效".to_string());
    }
    let crop_width = right - left;
    let crop_height = bottom - top;
    let mut pixels = Vec::with_capacity((crop_width * crop_height * 4) as usize);
    for y in top..bottom {
        let start = (y * row_pitch + left * 4) as usize;
        let end = start + (crop_width * 4) as usize;
        pixels.extend_from_slice(
            data.get(start..end)
                .ok_or_else(|| "OCR 标题区域超出帧缓冲区".to_string())?,
        );
    }
    Ok(OcrImage {
        pixels,
        width: crop_width,
        height: crop_height,
    })
}

#[cfg(windows)]
pub struct StageOcrRecognizer {
    catalog: Arc<StageCatalog>,
    engine: windows::Media::Ocr::OcrEngine,
}

#[cfg(windows)]
impl StageOcrRecognizer {
    pub fn new(catalog: Arc<StageCatalog>) -> Result<Self, String> {
        use windows::{Globalization::Language, Media::Ocr::OcrEngine, core::HSTRING};

        let language = Language::CreateLanguage(&HSTRING::from("zh-CN"))
            .map_err(|error| format!("创建 zh-CN OCR 语言失败：{error}"))?;
        if !OcrEngine::IsLanguageSupported(&language).unwrap_or(false) {
            return Err(
                "Windows 未安装简体中文 OCR 语言包，请手动选择关卡或安装语言功能".to_string(),
            );
        }
        let engine = OcrEngine::TryCreateFromLanguage(&language)
            .map_err(|error| format!("创建 Windows OCR 引擎失败：{error}"))?;
        Ok(Self { catalog, engine })
    }

    pub fn recognize(&self, image: OcrImage) -> StageRecognition {
        match self.recognize_text(image) {
            Ok(text) => self.catalog.match_ocr(&text),
            Err(warning) => StageRecognition {
                warning: Some(warning),
                ..StageRecognition::default()
            },
        }
    }

    fn recognize_text(&self, image: OcrImage) -> Result<String, String> {
        use windows::{
            Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap},
            Storage::Streams::DataWriter,
        };

        let width = i32::try_from(image.width).map_err(|_| "OCR 标题区域宽度过大".to_string())?;
        let height = i32::try_from(image.height).map_err(|_| "OCR 标题区域高度过大".to_string())?;
        let writer =
            DataWriter::new().map_err(|error| format!("创建 OCR 像素缓冲区失败：{error}"))?;
        writer
            .WriteBytes(&image.pixels)
            .map_err(|error| format!("写入 OCR 像素缓冲区失败：{error}"))?;
        let buffer = writer
            .DetachBuffer()
            .map_err(|error| format!("提交 OCR 像素缓冲区失败：{error}"))?;
        let bitmap =
            SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, width, height)
                .map_err(|error| format!("创建 OCR 图像失败：{error}"))?;
        self.engine
            .RecognizeAsync(&bitmap)
            .and_then(|operation| operation.join())
            .and_then(|result| result.Text())
            .map(|text| text.to_string_lossy())
            .map_err(|error| format!("Windows OCR 识别失败（未打包运行时可手动选择关卡）：{error}"))
    }
}

#[cfg(not(windows))]
pub struct StageOcrRecognizer {
    catalog: Arc<StageCatalog>,
}

#[cfg(not(windows))]
impl StageOcrRecognizer {
    pub fn new(catalog: Arc<StageCatalog>) -> Result<Self, String> {
        Ok(Self { catalog })
    }

    pub fn recognize(&self, _image: OcrImage) -> StageRecognition {
        let mut result = self.catalog.match_ocr("");
        result.warning = Some("关卡 OCR 仅支持 Windows 10/11".to_string());
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_crop_removes_source_stride_padding() {
        let width = 640;
        let height = 360;
        let row_pitch = width * 4 + 16;
        let frame = vec![255; (row_pitch * height) as usize];

        let crop = crop_title(&frame, width, height, row_pitch).unwrap();

        assert_eq!(crop.pixels.len(), (crop.width * crop.height * 4) as usize);
    }
}
