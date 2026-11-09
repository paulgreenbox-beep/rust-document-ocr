# Additional notices for Rust Document OCR for Windows

These notices supplement, not replace, the component notices included with the distribution. They apply to the reviewed Windows x64 candidate identified by executable SHA-256 `c6cddc7516d5e44b225cde2fabb35e534016e05a7c900774b087019e538dfc23`. This source packet contains no executable, models, or permission to publish the candidate.

- DjVu Rust components `djvu-bitmap`, `djvu-bzz`, `djvu-iff`, `djvu-iw44`, `djvu-jb2`, `djvu-pixmap` and `djvu-zp` 0.27.0: [MIT license and Lev Matyushkin copyright](notices/djvu-rs-0.27.0-LICENSE.txt).
- `docx-rs` 0.2.14: [MIT license](notices/docx-rs-0.2.14-LICENSE.txt). Its XML-to-JSON code includes an adaptation of Scott Cooper's XMLJSON-rs; retain the [XMLJSON-rs MIT notice](notices/xmlJSON-rs-LICENSE-MIT.txt).
- `pulp-wasm-simd-flag` 0.1.1: [MIT license](notices/pulp-wasm-simd-flag-0.1.1-LICENSE.txt).
- `zune-inflate` 0.2.54: Copyright (c) 2022 zune-image developers. [Zlib license option](notices/zune-image-LICENSE-ZLIB.txt). Its CRC lookup tables credit Eric Biggers; retain the [libdeflate MIT notice](notices/libdeflate-COPYING.txt).
- Native libjpeg-turbo 3.2.0: [LICENSE.md](notices/native-jpeg/LICENSE.md) and its required sibling [README.ijg](notices/native-jpeg/README.ijg). This software is based in part on the work of the Independent JPEG Group. These are distinct from the JPEG notices shipped with PDFium.
- liblzma from XZ Utils 5.8.3: [0BSD license](notices/xz-5.8.3-COPYING.0BSD.txt) for the library; retain the existing XZ COPYING.
- libarchive 3.8.7: [file-level copyright and license supplements](notices/libarchive-3.8.7-file-notices.txt) for source names identified in the reviewed link map and related headers. Retain libarchive's existing COPYING as well.
- Native OpenJPEG 2.5.4: [file-specific notices and generator credits](notices/openjpeg-2.5.4-file-notices.txt). Keep the existing native OpenJPEG LICENSE; these are separate from PDFium's notices.
- Rust standard-library artifacts from Rust 1.97.1: [copyright and component notices](notices/rust-1.97.1-COPYRIGHT-library.html), [MIT](notices/rust-1.97.1-LICENSE-MIT.txt) and [Apache-2.0](notices/rust-1.97.1-LICENSE-APACHE.txt).

The license alternatives above describe the proposed notice route for this candidate; they do not replace the terms of separately credited code. For PDFium, the patched HEIC decoder, and EG79, see their separate notices in a future approved distribution.
