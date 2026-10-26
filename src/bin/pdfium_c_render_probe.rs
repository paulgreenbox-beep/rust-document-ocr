//! Direct PDFium C-API render probe; it intentionally bypasses `pdfium-render`.
//!
//! The binary is diagnostic-only. It loads one explicitly named PDFium DLL, renders
//! page zero, and writes a PNG only after `FPDF_RenderPageBitmap()` returns.

#[cfg(feature = "pdfium-c-probe")]
mod probe {
    use std::{
        ffi::{c_char, c_int, c_uint, c_void, CString},
        io::{self, Write},
        path::PathBuf,
        ptr,
    };

    use clap::Parser;
    use image::{ImageBuffer, Rgba};
    use libloading::Library;

    const FPDF_BITMAP_BGRA: c_int = 4;
    const WHITE_BGRA: c_uint = 0xffff_ffff;

    type Document = *mut c_void;
    type Page = *mut c_void;
    type Bitmap = *mut c_void;

    type InitLibrary = unsafe extern "C" fn();
    type DestroyLibrary = unsafe extern "C" fn();
    type LoadDocument = unsafe extern "C" fn(*const c_char, *const c_char) -> Document;
    type CloseDocument = unsafe extern "C" fn(Document);
    type GetPageCount = unsafe extern "C" fn(Document) -> c_int;
    type LoadPage = unsafe extern "C" fn(Document, c_int) -> Page;
    type ClosePage = unsafe extern "C" fn(Page);
    type GetPageWidthF = unsafe extern "C" fn(Page) -> f32;
    type GetPageHeightF = unsafe extern "C" fn(Page) -> f32;
    type CreateBitmap = unsafe extern "C" fn(c_int, c_int, c_int, *mut c_void, c_int) -> Bitmap;
    type DestroyBitmap = unsafe extern "C" fn(Bitmap);
    type FillRect = unsafe extern "C" fn(Bitmap, c_int, c_int, c_int, c_int, c_uint);
    type RenderPageBitmap =
        unsafe extern "C" fn(Bitmap, Page, c_int, c_int, c_int, c_int, c_int, c_int);
    type GetBuffer = unsafe extern "C" fn(Bitmap) -> *mut c_void;
    type GetStride = unsafe extern "C" fn(Bitmap) -> c_int;
    type GetBitmapWidth = unsafe extern "C" fn(Bitmap) -> c_int;
    type GetBitmapHeight = unsafe extern "C" fn(Bitmap) -> c_int;
    type GetBitmapFormat = unsafe extern "C" fn(Bitmap) -> c_int;
    type GetLastError = unsafe extern "C" fn() -> c_uint;

    #[derive(Parser)]
    struct Args {
        #[arg(long)]
        pdfium_dll: PathBuf,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        png: PathBuf,
    }

    struct Api {
        init_library: InitLibrary,
        destroy_library: DestroyLibrary,
        load_document: LoadDocument,
        close_document: CloseDocument,
        get_page_count: GetPageCount,
        load_page: LoadPage,
        close_page: ClosePage,
        get_page_width_f: GetPageWidthF,
        get_page_height_f: GetPageHeightF,
        create_bitmap: CreateBitmap,
        destroy_bitmap: DestroyBitmap,
        fill_rect: FillRect,
        render_page_bitmap: RenderPageBitmap,
        get_buffer: GetBuffer,
        get_stride: GetStride,
        get_bitmap_width: GetBitmapWidth,
        get_bitmap_height: GetBitmapHeight,
        get_bitmap_format: GetBitmapFormat,
        get_last_error: GetLastError,
    }

    fn stage(name: &str) {
        println!("stage={name}");
        let _ = io::stdout().flush();
    }

    unsafe fn symbol<T: Copy>(library: &Library, name: &[u8]) -> Result<T, libloading::Error> {
        // SAFETY: caller supplies the exact C function-pointer signature for an exported symbol.
        Ok(*unsafe { library.get::<T>(name) }?)
    }

    unsafe fn resolve(library: &Library) -> Result<Api, libloading::Error> {
        // SAFETY: every symbol below is an exported PDFium C API function with the declared ABI.
        unsafe {
            Ok(Api {
                init_library: symbol(library, b"FPDF_InitLibrary\0")?,
                destroy_library: symbol(library, b"FPDF_DestroyLibrary\0")?,
                load_document: symbol(library, b"FPDF_LoadDocument\0")?,
                close_document: symbol(library, b"FPDF_CloseDocument\0")?,
                get_page_count: symbol(library, b"FPDF_GetPageCount\0")?,
                load_page: symbol(library, b"FPDF_LoadPage\0")?,
                close_page: symbol(library, b"FPDF_ClosePage\0")?,
                get_page_width_f: symbol(library, b"FPDF_GetPageWidthF\0")?,
                get_page_height_f: symbol(library, b"FPDF_GetPageHeightF\0")?,
                create_bitmap: symbol(library, b"FPDFBitmap_CreateEx\0")?,
                destroy_bitmap: symbol(library, b"FPDFBitmap_Destroy\0")?,
                fill_rect: symbol(library, b"FPDFBitmap_FillRect\0")?,
                render_page_bitmap: symbol(library, b"FPDF_RenderPageBitmap\0")?,
                get_buffer: symbol(library, b"FPDFBitmap_GetBuffer\0")?,
                get_stride: symbol(library, b"FPDFBitmap_GetStride\0")?,
                get_bitmap_width: symbol(library, b"FPDFBitmap_GetWidth\0")?,
                get_bitmap_height: symbol(library, b"FPDFBitmap_GetHeight\0")?,
                get_bitmap_format: symbol(library, b"FPDFBitmap_GetFormat\0")?,
                get_last_error: symbol(library, b"FPDF_GetLastError\0")?,
            })
        }
    }

    fn print_function_addresses(api: &Api, library_path: &std::path::Path) {
        println!("dll={}", library_path.display());
        println!(
            "process_arch={}",
            if cfg!(target_pointer_width = "64") {
                "x64"
            } else {
                "x86"
            }
        );
        println!("symbol.FPDF_InitLibrary=0x{:X}", api.init_library as usize);
        println!(
            "symbol.FPDF_LoadDocument=0x{:X}",
            api.load_document as usize
        );
        println!(
            "symbol.FPDF_RenderPageBitmap=0x{:X}",
            api.render_page_bitmap as usize
        );
    }

    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let args = Args::parse();
        stage("load_library");
        // SAFETY: the path is explicitly supplied; the library stays alive through cleanup.
        let library = unsafe { Library::new(&args.pdfium_dll) }?;

        stage("resolve_symbols");
        // SAFETY: the function signatures match PDFium's published C API.
        let api = unsafe { resolve(&library) }?;
        print_function_addresses(&api, &args.pdfium_dll);

        let mut document: Document = ptr::null_mut();
        let mut page: Page = ptr::null_mut();
        let mut bitmap: Bitmap = ptr::null_mut();

        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            stage("init_library");
            // SAFETY: resolved PDFium function pointer.
            unsafe { (api.init_library)() };

            stage("load_document");
            let input = CString::new(args.input.to_string_lossy().as_bytes())?;
            // SAFETY: input and null password are valid for the duration of this call.
            document = unsafe { (api.load_document)(input.as_ptr(), ptr::null()) };
            if document.is_null() {
                // SAFETY: resolved PDFium function pointer.
                let code = unsafe { (api.get_last_error)() };
                return Err(format!("FPDF_LoadDocument failed, last_error={code}").into());
            }

            stage("page_count");
            // SAFETY: document is a non-null PDFium document handle.
            let page_count = unsafe { (api.get_page_count)(document) };
            println!("pages={page_count}");
            if page_count <= 0 {
                return Err("document has no pages".into());
            }

            stage("load_page");
            // SAFETY: page zero is valid because page_count is positive.
            page = unsafe { (api.load_page)(document, 0) };
            if page.is_null() {
                return Err("FPDF_LoadPage(0) returned null".into());
            }

            stage("page_size");
            // SAFETY: page is a non-null PDFium page handle.
            let page_width = unsafe { (api.get_page_width_f)(page) };
            // SAFETY: page is a non-null PDFium page handle.
            let page_height = unsafe { (api.get_page_height_f)(page) };
            println!("page_width_points={page_width}");
            println!("page_height_points={page_height}");
            if !(page_width.is_finite()
                && page_height.is_finite()
                && page_width > 0.0
                && page_height > 0.0)
            {
                return Err("invalid page dimensions".into());
            }

            let bitmap_width = 800_i32;
            let bitmap_height =
                ((bitmap_width as f32 * page_height / page_width).round() as i32).max(1);
            stage("create_bitmap");
            // SAFETY: PDFium allocates the bitmap buffer because buffer is null.
            bitmap = unsafe {
                (api.create_bitmap)(
                    bitmap_width,
                    bitmap_height,
                    FPDF_BITMAP_BGRA,
                    ptr::null_mut(),
                    0,
                )
            };
            if bitmap.is_null() {
                return Err("FPDFBitmap_CreateEx returned null".into());
            }

            stage("fill_bitmap");
            // SAFETY: bitmap is valid and the rectangle is within its bounds.
            unsafe { (api.fill_rect)(bitmap, 0, 0, bitmap_width, bitmap_height, WHITE_BGRA) };

            stage("render_page");
            // SAFETY: page and bitmap are valid; flags and rotation are deliberately zero.
            unsafe {
                (api.render_page_bitmap)(bitmap, page, 0, 0, bitmap_width, bitmap_height, 0, 0)
            };

            stage("read_bitmap");
            // SAFETY: bitmap remains valid until cleanup below.
            let width = unsafe { (api.get_bitmap_width)(bitmap) };
            // SAFETY: bitmap remains valid until cleanup below.
            let height = unsafe { (api.get_bitmap_height)(bitmap) };
            // SAFETY: bitmap remains valid until cleanup below.
            let stride = unsafe { (api.get_stride)(bitmap) };
            // SAFETY: bitmap remains valid until cleanup below.
            let format = unsafe { (api.get_bitmap_format)(bitmap) };
            println!("bitmap_width={width}");
            println!("bitmap_height={height}");
            println!("bitmap_stride={stride}");
            println!("bitmap_format={format}");

            if width <= 0 || height <= 0 {
                return Err("PDFium returned non-positive bitmap dimensions".into());
            }
            if width != bitmap_width || height != bitmap_height {
                return Err(format!(
                    "PDFium returned unexpected bitmap size {width}x{height}, expected {bitmap_width}x{bitmap_height}"
                )
                .into());
            }
            if format != FPDF_BITMAP_BGRA {
                return Err(format!(
                    "PDFium returned unexpected bitmap format {format}, expected {FPDF_BITMAP_BGRA}"
                )
                .into());
            }
            if stride <= 0 {
                return Err("PDFium returned a non-positive bitmap stride".into());
            }

            let row_bytes = usize::try_from(width)?
                .checked_mul(4)
                .ok_or("packed row size overflow")?;
            let stride = usize::try_from(stride)?;
            let height_usize = usize::try_from(height)?;
            if stride < row_bytes {
                return Err(format!(
                    "invalid bitmap stride {stride}, expected at least {row_bytes}"
                )
                .into());
            }
            let byte_len = stride
                .checked_mul(height_usize)
                .ok_or("bitmap size overflow")?;
            if byte_len > isize::MAX as usize {
                return Err("PDFium bitmap buffer exceeds the maximum slice size".into());
            }
            let output_len = row_bytes
                .checked_mul(height_usize)
                .ok_or("packed bitmap size overflow")?;
            if output_len > isize::MAX as usize {
                return Err("packed bitmap exceeds the maximum allocation size".into());
            }

            // SAFETY: bitmap is live; the pointer is validated before constructing a slice.
            let buffer = unsafe { (api.get_buffer)(bitmap) };
            if buffer.is_null() {
                return Err("PDFium returned a null bitmap buffer".into());
            }
            // SAFETY: PDFium allocated a buffer at least stride * height bytes for this bitmap.
            let source = unsafe { std::slice::from_raw_parts(buffer.cast::<u8>(), byte_len) };

            let mut rgba = Vec::with_capacity(output_len);
            let mut channel_min = u8::MAX;
            let mut channel_max = u8::MIN;
            let mut all_white = true;
            for row in 0..height_usize {
                let source_row = &source[row * stride..row * stride + row_bytes];
                for pixel in source_row.chunks_exact(4) {
                    let [blue, green, red, alpha] = [pixel[0], pixel[1], pixel[2], pixel[3]];
                    channel_min = channel_min.min(blue).min(green).min(red).min(alpha);
                    channel_max = channel_max.max(blue).max(green).max(red).max(alpha);
                    all_white &= red == 255 && green == 255 && blue == 255;
                    rgba.extend_from_slice(&[red, green, blue, alpha]);
                }
            }
            println!("pixel_range={channel_min}..{channel_max}");
            println!("all_white={all_white}");

            stage("save_png");
            let image = ImageBuffer::<Rgba<u8>, _>::from_raw(width as u32, height as u32, rgba)
                .ok_or("failed to construct RGBA image from PDFium bitmap")?;
            image.save(&args.png)?;
            println!("png={}", args.png.display());
            Ok(())
        })();

        stage("cleanup");
        // SAFETY: each handle is released at most once, in reverse ownership order.
        unsafe {
            if !bitmap.is_null() {
                (api.destroy_bitmap)(bitmap);
            }
            if !page.is_null() {
                (api.close_page)(page);
            }
            if !document.is_null() {
                (api.close_document)(document);
                (api.destroy_library)();
            }
        }
        stage("done");
        result
    }
}

#[cfg(feature = "pdfium-c-probe")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    probe::run()
}

#[cfg(not(feature = "pdfium-c-probe"))]
fn main() {
    eprintln!("enable the pdfium-c-probe feature");
}
