//! Opt-in Word 97–2003 conversion. No converter is discovered or installed.

use std::{
    fs,
    io::{self, Read, Write},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const MAX_DOC_BYTES: u64 = 128 * 1024 * 1024;
const CONVERSION_TIMEOUT: Duration = Duration::from_secs(120);

/// Convert an existing DOCX using an explicitly selected, trusted LibreOffice
/// executable. Keep the input, refuse existing outputs, and validate the
/// result before making it visible under the requested name.
pub fn convert_docx_to_doc(soffice: &Path, docx: &Path, doc: &Path) -> io::Result<()> {
    if !soffice.is_absolute() || !fs::metadata(soffice)?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "supply the absolute path to a trusted LibreOffice executable",
        ));
    }
    let input = docx.canonicalize()?;
    if !input.is_file() || !extension(&input, "docx") || !extension(doc, "doc") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected an existing .docx input and a .doc output",
        ));
    }
    match fs::symlink_metadata(doc) {
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "DOC output already exists",
            ))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let parent = doc
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let work = tempfile::Builder::new()
        .prefix(".ocr-doc-")
        .tempdir_in(parent.canonicalize()?)?;
    let profile = work.path().join("profile");
    let converted_dir = work.path().join("converted");
    fs::create_dir(&converted_dir)?;
    let mut child = Command::new(soffice)
        .arg(format!("-env:UserInstallation={}", file_url(&profile)?))
        .args([
            "--headless",
            "--nologo",
            "--nodefault",
            "--norestore",
            "--convert-to",
            "doc:MS Word 97",
            "--outdir",
        ])
        .arg(&converted_dir)
        .arg(&input)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(status)) => {
                return Err(io::Error::other(format!(
                    "LibreOffice conversion failed ({status})"
                )))
            }
            Ok(None) if start.elapsed() < CONVERSION_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(50))
            }
            pending => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(pending.err().unwrap_or_else(|| {
                    io::Error::new(io::ErrorKind::TimedOut, "LibreOffice conversion timed out")
                }));
            }
        }
    }
    let name = input
        .file_stem()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "DOCX file name missing"))?;
    let converted = converted_dir.join(name).with_extension("doc");
    let meta = fs::symlink_metadata(&converted)?;
    if !meta.file_type().is_file() || meta.len() > MAX_DOC_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "converted DOC must be a bounded regular file",
        ));
    }
    let bytes = fs::read(&converted)?;
    validate_doc(&bytes)?;
    let mut candidate = tempfile::NamedTempFile::new_in(parent)?;
    candidate.write_all(&bytes)?;
    candidate.as_file().sync_all()?;
    candidate.persist_noclobber(doc)?;
    Ok(())
}

fn extension(file: &Path, expected: &str) -> bool {
    file.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case(expected))
}

fn file_url(path: &Path) -> io::Result<String> {
    let source = path.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid Unicode in converter profile path",
        )
    })?;
    #[cfg(windows)]
    let source = source
        .strip_prefix(r"\\?\")
        .unwrap_or(source)
        .replace('\\', "/");
    #[cfg(windows)]
    if source.starts_with('/') || source.as_bytes().get(1) != Some(&b':') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "DOC conversion requires a local Windows path",
        ));
    }
    let mut uri = if source.starts_with('/') {
        "file://".to_owned()
    } else {
        "file:///".to_owned()
    };
    for byte in source.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/:".contains(&byte) {
            uri.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            write!(&mut uri, "%{byte:02X}").expect("format to String");
        }
    }
    Ok(uri)
}

/// Validate the compound container, binary Word header and the referenced
/// table stream. This detects renamed ZIP/RTF and unrelated OLE files; it
/// does not prove that the application preserved every DOCX element.
pub fn validate_doc(bytes: &[u8]) -> io::Result<()> {
    let mut cfb = cfb::CompoundFile::open_strict(io::Cursor::new(bytes))?;
    let mut fib = [0_u8; 32];
    cfb.open_stream("/WordDocument")?.read_exact(&mut fib)?;
    let version = u16::from_le_bytes([fib[2], fib[3]]);
    if fib[..2] != [0xEC, 0xA5] || !matches!(version, 0x00C1 | 0x00D9 | 0x0101 | 0x010C | 0x0112) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "expected a Word 97–2003 FIB",
        ));
    }
    let flags = u16::from_le_bytes([fib[10], fib[11]]);
    if flags & 0x0101 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "expected an unencrypted Word document, not a template",
        ));
    }
    let table = if flags & 0x0200 != 0 {
        "/1Table"
    } else {
        "/0Table"
    };
    cfb.open_stream(table)?.read_exact(&mut [0])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_renamed_zip_rtf_and_ole_without_word_streams() {
        assert!(validate_doc(b"PK\x03\x04").is_err());
        assert!(validate_doc(br"{\rtf1  fake}").is_err());
        let ole = cfb::CompoundFile::create(io::Cursor::new(Vec::new())).unwrap();
        assert!(validate_doc(ole.into_inner().get_ref()).is_err());
        let mut ole = cfb::CompoundFile::create(io::Cursor::new(Vec::new())).unwrap();
        let mut fib = [0_u8; 32];
        fib[..4].copy_from_slice(&[0xEC, 0xA5, 0xC1, 0]);
        ole.create_stream("/WordDocument")
            .unwrap()
            .write_all(&fib)
            .unwrap();
        ole.flush().unwrap();
        let mut ole = cfb::CompoundFile::open(ole.into_inner()).unwrap();
        assert!(!ole.is_stream("/0Table"));
        ole.create_stream("/0Table")
            .unwrap()
            .write_all(&[0])
            .unwrap();
        ole.flush().unwrap();
        assert!(validate_doc(ole.into_inner().get_ref()).is_ok());
    }

    #[test]
    fn profile_uri_escapes_spaces_hash_and_cyrillic() {
        let source = if cfg!(windows) {
            "C:\\A B\\#Я"
        } else {
            "/A B/#Я"
        };
        let expected = if cfg!(windows) {
            "file:///C:/A%20B/%23%D0%AF"
        } else {
            "file:///A%20B/%23%D0%AF"
        };
        assert_eq!(file_url(Path::new(source)).unwrap(), expected);
    }
}
