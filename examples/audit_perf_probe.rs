//! Bounded synthetic diagnostics for the OCR image conversion path and
//! repeated PDFium source operations. The output contains timings and sizes
//! only: no source paths, recognized text, or document metadata are emitted.
//! Run only with the committed synthetic PDF fixture. Peak process memory is
//! sampled externally during the worker-scaling phase.
//!
//! Run with `cargo run --offline --locked --features native --example audit_perf_probe`.

#[cfg(not(feature = "native"))]
fn main() {
    println!(r#"{{"status":"native_feature_required"}}"#);
}

#[cfg(feature = "native")]
fn main() {
    match run() {
        Ok(report) => println!("{report}"),
        Err(phase) => {
            println!("{{\"status\":\"error\",\"phase\":\"{phase}\"}}");
            std::process::exit(2);
        }
    }
}

#[cfg(feature = "native")]
fn run() -> Result<serde_json::Value, &'static str> {
    use std::{path::PathBuf, time::Instant};

    use leptess::{leptonica::pix_read_mem, tesseract::TessApi};
    use ocr_pipeline::page_source::PageSource;
    use ocr_pipeline::{OcrEngine, OcrRequest, PdfiumPageSource, TesseractOcrEngine};

    const SAMPLE_COUNT: usize = 30;
    const WARMUPS: usize = 3;
    const MAX_SYNTHETIC_PIXELS: u32 = 4_000_000;
    const WORKER_IMAGE_PIXELS: u32 = 2_000_000;
    let worker_counts = selected_worker_counts()?;
    if std::env::var_os("OCR_AUDIT_WORKERS").is_some() {
        return Ok(serde_json::json!({
            "status": "ok",
            "worker_conversion_scaling": worker_scaling(WORKER_IMAGE_PIXELS, &worker_counts)
                .map_err(|_| "worker_probe_failed")?,
            "scope": "conversion only; no OCR or full page-worker memory claim",
        }));
    }

    let mut conversion = Vec::new();
    let mut synthetic_images = Vec::new();
    for (width, height) in [(1_000, 1_000), (2_000, 2_000)] {
        let image = synthetic_text_image(width, height);
        let pixel_count = width * height;
        if pixel_count > MAX_SYNTHETIC_PIXELS {
            return Err("synthetic_limit_exceeded");
        }
        let clone_ms = measure(SAMPLE_COUNT, WARMUPS, || {
            std::hint::black_box(image.clone());
        });
        let encode_ms = measure(SAMPLE_COUNT, WARMUPS, || {
            std::hint::black_box(encode_png_borrowed(&image).expect("encode synthetic PNG"));
        });
        let clone_encode_ms = measure(SAMPLE_COUNT, WARMUPS, || {
            std::hint::black_box(encode_png(&image).expect("encode cloned synthetic PNG"));
        });
        let encoded = encode_png(&image).map_err(|_| "png_encode_failed")?;
        let decode_ms = measure(SAMPLE_COUNT, WARMUPS, || {
            std::hint::black_box(pix_read_mem(&encoded).expect("decode synthetic PNG"));
        });
        conversion.push(serde_json::json!({
            "width": width,
            "height": height,
            "pixels": pixel_count,
            "png_bytes": encoded.len(),
            "clone_ms": summary(clone_ms),
            "png_encode_borrowed_ms": summary(encode_ms),
            "clone_plus_png_encode_ms": summary(clone_encode_ms),
            "leptonica_pix_read_mem_ms": summary(decode_ms),
        }));
        synthetic_images.push(image);
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let tessdata = std::env::var_os("TESSDATA_PREFIX")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("runtime/tessdata"));
    let tessdata_str = tessdata.to_str().ok_or("tessdata_path_not_utf8")?;
    let initialization_started = Instant::now();
    let mut raw_api = TessApi::new(Some(tessdata_str), "eng").map_err(|_| "tessapi_init_failed")?;
    let initialization_ms = initialization_started.elapsed().as_secs_f64() * 1_000.0;
    let mut raw_recognition = Vec::new();
    for image in &synthetic_images {
        let encoded = encode_png(image).map_err(|_| "png_encode_failed")?;
        let pix = pix_read_mem(&encoded).map_err(|_| "leptonica_decode_failed")?;
        raw_api.set_image(&pix);
        raw_api.set_source_resolution(300);
        if raw_api.recognize() != 0 {
            return Err("tessapi_warmup_failed");
        }
        let mut samples = Vec::new();
        for _ in 0..3 {
            raw_api.set_image(&pix);
            raw_api.set_source_resolution(300);
            let started = Instant::now();
            if raw_api.recognize() != 0 {
                return Err("tessapi_recognition_failed");
            }
            samples.push(started.elapsed().as_secs_f64() * 1_000.0);
        }
        raw_recognition.push(serde_json::json!({
            "pixels": image.width() * image.height(),
            "already_decoded_pix": true,
            "recognize_ms": summary(samples),
        }));
    }
    drop(raw_api);

    let engine = TesseractOcrEngine::new_lazy_with_tessdata("eng", Some(&tessdata))
        .map_err(|_| "tesseract_init_failed")?;
    let mut tesseract = Vec::new();
    for image in &synthetic_images {
        let dpi = 300;
        engine
            .begin_page()
            .map_err(|_| "tesseract_page_reset_failed")?;
        let started = Instant::now();
        std::hint::black_box(
            engine
                .recognize_text_and_words_with_request(image, OcrRequest::page(dpi))
                .map_err(|_| "tesseract_cold_recognition_failed")?,
        );
        let cold_call_ms = started.elapsed().as_secs_f64() * 1_000.0;
        let mut samples = Vec::new();
        for _ in 0..3 {
            let started = Instant::now();
            std::hint::black_box(
                engine
                    .recognize_text_and_words_with_request(image, OcrRequest::page(dpi))
                    .map_err(|_| "tesseract_recognition_failed")?,
            );
            samples.push(started.elapsed().as_secs_f64() * 1_000.0);
        }
        tesseract.push(serde_json::json!({
            "pixels": image.width() * image.height(),
            "cold_first_call_ms": cold_call_ms,
            "steady_full_engine_call_ms": summary(samples),
            "note": "full-engine calls include PNG encode/decode; cold first call also initializes Tesseract",
        }));
    }

    // Each worker handles only a 2 MP synthetic image. The root process may
    // sample peak RSS externally while this bounded phase runs.
    let worker_scaling =
        worker_scaling(WORKER_IMAGE_PIXELS, &worker_counts).map_err(|_| "worker_probe_failed")?;

    let fixture = root.join("examples/documents/synthetic-text-table.pdf");
    let pdfium_dll = std::env::var_os("OCR_TEST_PDFIUM_LIBRARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("runtime/pdfium/7881/bin/pdfium.dll"));
    let open_started = Instant::now();
    let mut source = PdfiumPageSource::open_with_pdfium_dll(&fixture, 300.0, Some(&pdfium_dll))
        .map_err(|_| "pdfium_open_failed")?;
    let open_ms = open_started.elapsed().as_secs_f64() * 1_000.0;
    let page_count = source
        .total_pages()
        .map_err(|_| "pdfium_page_count_failed")?;
    if page_count == 0 {
        return Err("pdfium_fixture_has_no_pages");
    }

    let mut native_extract_ms = Vec::new();
    let mut render_ms = Vec::new();
    for _ in 0..3 {
        let started = Instant::now();
        std::hint::black_box(
            source
                .extract_native_text_layer(1)
                .map_err(|_| "pdfium_native_extract_failed")?,
        );
        native_extract_ms.push(started.elapsed().as_secs_f64() * 1_000.0);

        let started = Instant::now();
        std::hint::black_box(
            source
                .render_page_with_max_pixels(1, WORKER_IMAGE_PIXELS as usize)
                .map_err(|_| "pdfium_render_failed")?,
        );
        render_ms.push(started.elapsed().as_secs_f64() * 1_000.0);
    }

    Ok(serde_json::json!({
        "status": "ok",
        "sample_count": SAMPLE_COUNT,
        "warmups": WARMUPS,
        "conversion": conversion,
        "tesseract_initialization_ms": initialization_ms,
        "tessapi_raw_recognition": raw_recognition,
        "tesseract": tesseract,
        "worker_conversion_scaling": worker_scaling,
        "worker_counts": worker_counts,
        "pdfium": {
            "source_open_ms": open_ms,
            "native_extract_reopen_ms": summary(native_extract_ms),
            "render_reopen_ms": summary(render_ms),
            "fixture_id": "committed_synthetic_text_table",
        },
        "memory_note": "sample process peak RSS externally during worker_conversion_scaling",
    }))
}

#[cfg(feature = "native")]
fn encode_png(image: &image::GrayImage) -> Result<Vec<u8>, image::ImageError> {
    use std::io::Cursor;

    let mut cursor = Cursor::new(Vec::new());
    image::DynamicImage::ImageLuma8(image.clone())
        .write_to(&mut cursor, image::ImageFormat::Png)?;
    Ok(cursor.into_inner())
}

#[cfg(feature = "native")]
fn encode_png_borrowed(image: &image::GrayImage) -> Result<Vec<u8>, image::ImageError> {
    use image::ImageEncoder;
    use std::io::Cursor;

    let mut cursor = Cursor::new(Vec::new());
    image::codecs::png::PngEncoder::new(&mut cursor).write_image(
        image.as_raw(),
        image.width(),
        image.height(),
        image::ExtendedColorType::L8,
    )?;
    Ok(cursor.into_inner())
}

#[cfg(feature = "native")]
fn synthetic_text_image(width: u32, height: u32) -> image::GrayImage {
    let mut image = image::GrayImage::from_pixel(width, height, image::Luma([255]));
    let scale = (width / 250).clamp(3, 10);
    draw_text(&mut image, 36, 42, "TEST 123", scale);

    let table_top = height / 3;
    let table_bottom = table_top + height / 4;
    let left = width / 12;
    let right = width - left;
    for y in [
        table_top,
        table_top + (table_bottom - table_top) / 2,
        table_bottom,
    ] {
        draw_line(&mut image, left, y, right, y);
    }
    for x in [
        left,
        left + (right - left) / 3,
        left + 2 * (right - left) / 3,
        right,
    ] {
        draw_line(&mut image, x, table_top, x, table_bottom);
    }
    image
}

#[cfg(feature = "native")]
fn draw_text(image: &mut image::GrayImage, x: u32, y: u32, text: &str, scale: u32) {
    for (index, character) in text.chars().enumerate() {
        let Some(rows) = glyph(character) else {
            continue;
        };
        for (row, bits) in rows.iter().enumerate() {
            for column in 0..5 {
                if bits & (1 << (4 - column)) != 0 {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let px = x + index as u32 * 6 * scale + column * scale + dx;
                            let py = y + row as u32 * scale + dy;
                            if px < image.width() && py < image.height() {
                                image.put_pixel(px, py, image::Luma([0]));
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(feature = "native")]
fn glyph(character: char) -> Option<[u8; 7]> {
    Some(match character {
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        _ => return None,
    })
}

#[cfg(feature = "native")]
fn draw_line(image: &mut image::GrayImage, x0: u32, y0: u32, x1: u32, y1: u32) {
    if x0 == x1 {
        for y in y0..=y1 {
            image.put_pixel(x0, y, image::Luma([0]));
        }
    } else {
        for x in x0..=x1 {
            image.put_pixel(x, y0, image::Luma([0]));
        }
    }
}

#[cfg(feature = "native")]
fn selected_worker_counts() -> Result<Vec<usize>, &'static str> {
    let Some(value) = std::env::var_os("OCR_AUDIT_WORKERS") else {
        return Ok(vec![1, 2, 4, 8, 16]);
    };
    let value = value.to_str().ok_or("worker_count_not_utf8")?;
    let workers = value.parse::<usize>().map_err(|_| "invalid_worker_count")?;
    if [1, 2, 4, 8, 16].contains(&workers) {
        Ok(vec![workers])
    } else {
        Err("worker_count_must_be_one_of_1_2_4_8_16")
    }
}

#[cfg(feature = "native")]
fn worker_scaling(
    pixels: u32,
    worker_counts: &[usize],
) -> Result<Vec<serde_json::Value>, &'static str> {
    use image::{GrayImage, Luma};
    use leptess::leptonica::pix_read_mem;
    use std::{sync::Barrier, time::Instant};

    let mut results = Vec::new();
    for &workers in worker_counts {
        let barrier = std::sync::Arc::new(Barrier::new(workers));
        let started = Instant::now();
        std::thread::scope(|scope| -> Result<(), &'static str> {
            let mut handles = Vec::with_capacity(workers);
            for worker in 0..workers {
                let barrier = std::sync::Arc::clone(&barrier);
                handles.push(scope.spawn(move || -> Result<(), &'static str> {
                    let width = 1_000;
                    let height = pixels / width;
                    let mut image = GrayImage::from_pixel(width, height, Luma([255]));
                    let marker = (worker as u8).wrapping_mul(17);
                    for (index, pixel) in image.pixels_mut().enumerate().step_by(4_096) {
                        pixel[0] = marker.wrapping_add(index as u8);
                    }
                    barrier.wait();
                    let encoded = encode_png(&image).map_err(|_| "worker_png_encode_failed")?;
                    let pix = pix_read_mem(&encoded).map_err(|_| "worker_pix_decode_failed")?;
                    std::hint::black_box((image, encoded, pix));
                    Ok(())
                }));
            }
            for handle in handles {
                handle.join().map_err(|_| "worker_panicked")??;
            }
            Ok(())
        })?;
        let elapsed_ms = started.elapsed().as_secs_f64() * 1_000.0;
        results.push(serde_json::json!({
            "workers": workers,
            "pixels_per_worker": pixels,
            "elapsed_ms": elapsed_ms,
        }));
    }
    Ok(results)
}

#[cfg(feature = "native")]
fn measure(count: usize, warmups: usize, mut operation: impl FnMut()) -> Vec<f64> {
    for _ in 0..warmups {
        operation();
    }
    let count = count.max(1);
    let mut samples = Vec::with_capacity(count);
    for _ in 0..count {
        let started = std::time::Instant::now();
        operation();
        samples.push(started.elapsed().as_secs_f64() * 1_000.0);
    }
    samples
}

#[cfg(feature = "native")]
fn summary(mut samples: Vec<f64>) -> serde_json::Value {
    samples.sort_by(f64::total_cmp);
    let percentile = |fraction: f64| {
        let index = ((samples.len().saturating_sub(1)) as f64 * fraction).round() as usize;
        samples.get(index).copied().unwrap_or_default()
    };
    serde_json::json!({
        "samples": samples.len(),
        "p50": percentile(0.50),
        "p95": percentile(0.95),
    })
}
