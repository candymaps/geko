//! Embeds the dashboard (`ui/dist`, which `ci/ui.sh` builds) in the binary, each file raw and
//! gzipped, so that serving one is a copy of its bytes with no compression per request. Without
//! `ui/dist` the table is empty and the server answers a page that says to build it.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::{env, fs};

fn files(dir: &Path, found: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir).expect("readable ui/dist").map(|e| e.expect("an entry").path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            files(&path, found);
        } else {
            found.push(path);
        }
    }
}

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("set by cargo")).join("../../ui/dist");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("set by cargo"));
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", root.display());

    let mut table = String::from("pub static ASSETS: &[Asset] = &[\n");
    if root.is_dir() {
        let mut found = Vec::new();
        files(&root, &mut found);
        for (i, file) in found.iter().enumerate() {
            let relative = file.strip_prefix(&root).expect("under ui/dist").to_string_lossy().replace('\\', "/");
            let raw = fs::read(file).expect("readable file");
            let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
            gzip.write_all(&raw).expect("in memory");
            let gz = out.join(format!("ui-{i}.gz"));
            fs::write(&gz, gzip.finish().expect("in memory")).expect("writable OUT_DIR");
            let path = if relative == "index.html" { "/".to_owned() } else { format!("/{relative}") };
            let (raw_path, gz_path) = (file.to_str().expect("a UTF-8 path"), gz.to_str().expect("a UTF-8 path"));
            writeln!(table, "    Asset {{ path: {path:?}, mime: {:?}, raw: include_bytes!({raw_path:?}), gz: include_bytes!({gz_path:?}) }},", mime(&relative))
                .expect("in memory");
        }
    } else {
        println!("cargo:warning=ui/dist does not exist: the dashboard is not embedded (run make ui)");
    }
    table.push_str("];\n");
    fs::write(out.join("ui_assets.rs"), table).expect("writable OUT_DIR");
}
