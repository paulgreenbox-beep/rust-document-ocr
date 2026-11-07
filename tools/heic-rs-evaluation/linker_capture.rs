//! Research-only final MSVC link recorder, compiled separately with rustc.
//! Raw private evidence must never be committed: it contains local input paths.
use std::{
    env,
    error::Error,
    ffi::OsString,
    fs::{self, OpenOptions},
    io::Write,
    os::windows::ffi::OsStrExt,
    path::PathBuf,
    process::{self, Command},
};

fn json_string(value: &str) -> String {
    let mut result = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            ch if ch <= '\u{1f}' => result.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => result.push(ch),
        }
    }
    result.push('"');
    result
}

fn write_new(path: PathBuf, data: &[u8]) -> Result<(), Box<dyn Error>> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(data)?;
    file.flush()?;
    Ok(())
}

fn run() -> Result<i32, Box<dyn Error>> {
    let linker = PathBuf::from(env::var_os("HEIC_REAL_LINKER").ok_or("HEIC_REAL_LINKER absent")?);
    let directory = PathBuf::from(
        env::var_os("HEIC_LINK_EVIDENCE_DIR").ok_or("HEIC_LINK_EVIDENCE_DIR absent")?,
    );
    if !linker.is_absolute() || !directory.is_absolute() || !directory.is_dir() {
        return Err("existing absolute linker/evidence paths required".into());
    }
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let entries: Vec<String> = args
        .iter()
        .enumerate()
        .map(|(index, argument)| {
            let units: Vec<u16> = argument.encode_wide().collect();
            format!(
                "{{\"index\":{index},\"display\":{},\"utf16\":{units:?}}}",
                json_string(&argument.to_string_lossy())
            )
        })
        .collect();
    let invocation = format!(
        "{{\"cwd\":{},\"linker\":{},\"arguments\":[{}]}}\n",
        json_string(&env::current_dir()?.to_string_lossy()),
        json_string(&linker.to_string_lossy()),
        entries.join(",")
    );
    write_new(directory.join("invocation.json"), invocation.as_bytes())?;
    // Retain exact BOM/bytes before rustc deletes its transient response file.
    // Decode/expand these separately during analysis, never to invoke the linker.
    for (index, argument) in args.iter().enumerate() {
        if let Some(path) = argument.to_str().and_then(|value| value.strip_prefix('@')) {
            let path = PathBuf::from(path.trim_matches('"'));
            let bytes = fs::read(&path)?;
            write_new(directory.join(format!("argument-{index}.rsp.raw")), &bytes)?;
        }
    }
    let output = Command::new(&linker).args(&args).output()?;
    write_new(directory.join("link.stdout.raw"), &output.stdout)?;
    write_new(directory.join("link.stderr.raw"), &output.stderr)?;
    let code = output.status.code().unwrap_or(1);
    write_new(
        directory.join("exit.json"),
        format!("{{\"exit_code\":{code}}}\n").as_bytes(),
    )?;
    if !output.status.success() {
        std::io::stdout().write_all(&output.stdout)?;
        std::io::stderr().write_all(&output.stderr)?;
    }
    Ok(code)
}

fn main() {
    match run() {
        Ok(code) => process::exit(code),
        Err(error) => {
            eprintln!("research linker capture failed: {error}");
            process::exit(1);
        }
    }
}
