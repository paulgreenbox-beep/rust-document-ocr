//! Runtime configuration assembled from CLI flags.

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub input: PathBuf,
    pub output_dir: PathBuf,
    /// Optional explicit PDFium runtime. Supplying this avoids any dependency
    /// on the build checkout's runtime layout or on global `PATH`.
    pub pdfium_dll_path: Option<PathBuf>,
    pub dpi: u32,
    pub grid_dpi: u32,
    pub languages: String,
    pub language_routing: crate::language::LanguageRouting,
    /// Optional explicit tessdata directory. It is passed to Tesseract without
    /// mutating `TESSDATA_PREFIX` globally.
    pub tessdata_path: Option<PathBuf>,
    pub page_limit: Option<u32>,
    pub parallelism: usize,
    pub enable_cell_ocr_fallback: bool,
}

impl Config {
    pub fn validate(&self) -> Result<(), String> {
        if !self.input.exists() {
            return Err(format!("Input file not found: {}", self.input.display()));
        }
        if let Some(path) = &self.pdfium_dll_path {
            if !path.is_file() {
                return Err(format!("PDFium DLL not found: {}", path.display()));
            }
        }
        if self.dpi < 150 || self.dpi > 400 {
            return Err(format!("DPI must be between 150 and 400, got {}", self.dpi));
        }
        if self.grid_dpi < 72 || self.grid_dpi > 200 {
            return Err(format!(
                "grid DPI must be between 72 and 200, got {}",
                self.grid_dpi
            ));
        }
        if self.languages.trim().is_empty() {
            return Err("OCR languages must not be empty".into());
        }
        if let Some(path) = &self.tessdata_path {
            if !path.is_dir() {
                return Err(format!("tessdata directory not found: {}", path.display()));
            }
        }
        if self.parallelism == 0 {
            return Err("parallelism must be greater than zero".into());
        }
        if self.parallelism > 64 {
            return Err("parallelism must not exceed 64".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(dpi: u32, parallelism: usize) -> Config {
        Config {
            input: PathBuf::from("Cargo.toml"), // exists in project root
            output_dir: PathBuf::from("nonexistent"),
            pdfium_dll_path: None,
            dpi,
            grid_dpi: 100,
            languages: "rus+eng".into(),
            language_routing: crate::language::LanguageRouting::Configured,
            tessdata_path: None,
            page_limit: None,
            parallelism,
            enable_cell_ocr_fallback: true,
        }
    }

    #[test]
    fn valid_config_passes() {
        let c = make_config(200, 1);
        assert!(c.validate().is_ok());
    }

    #[test]
    fn dpi_below_150_fails() {
        let c = make_config(100, 1);
        assert!(c.validate().unwrap_err().contains("DPI"));
    }

    #[test]
    fn dpi_above_400_fails() {
        let c = make_config(500, 1);
        assert!(c.validate().unwrap_err().contains("DPI"));
    }

    #[test]
    fn parallelism_zero_fails() {
        let c = make_config(200, 0);
        assert!(c.validate().is_err());
    }

    #[test]
    fn parallelism_up_to_eight_is_valid() {
        let c = make_config(200, 4);
        assert!(c.validate().is_ok());
    }

    #[test]
    fn missing_explicit_pdfium_dll_fails_before_processing() {
        let mut c = make_config(200, 1);
        c.pdfium_dll_path = Some(PathBuf::from("missing-pdfium.dll"));
        assert!(c.validate().unwrap_err().contains("PDFium DLL"));
    }

    #[test]
    fn empty_languages_fail_before_processing() {
        let mut c = make_config(300, 1);
        c.languages.clear();
        assert!(c.validate().unwrap_err().contains("languages"));
    }

    #[test]
    fn missing_explicit_tessdata_fails_before_processing() {
        let mut c = make_config(300, 1);
        c.tessdata_path = Some(PathBuf::from("missing-tessdata"));
        assert!(c.validate().unwrap_err().contains("tessdata"));
    }
}
