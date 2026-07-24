use std::{env, fs, path::Path};

/// Collects whatever SRS and Lagrange payloads have been exported into
/// `assets/precomputed/` and emits a table of them. The payloads are large
/// and reproducible, so they are not committed: when the directory is empty
/// the table is empty too and the app simply recomputes them on first use.
///
/// Names follow `srs-<curve>.bin` and `lagrange-<curve>-<domain_log2>.bin`,
/// as written by the `export_srs_payloads` test.
fn main() {
    let assets = Path::new("assets/precomputed");
    println!("cargo:rerun-if-changed=assets/precomputed");

    let mut entries = Vec::new();
    if let Ok(dir) = fs::read_dir(assets) {
        for entry in dir.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(stem) = name.strip_suffix(".bin") else {
                continue;
            };
            let path = entry.path().canonicalize().expect("payload path");
            let path = path.to_string_lossy().into_owned();
            if let Some(curve) = stem.strip_prefix("srs-") {
                entries.push(format!("({curve:?}, None, include_bytes!({path:?}))"));
            } else if let Some(rest) = stem.strip_prefix("lagrange-") {
                if let Some((curve, domain_log2)) = rest.rsplit_once('-') {
                    entries.push(format!(
                        "({curve:?}, Some({domain_log2}), include_bytes!({path:?}))"
                    ));
                }
            }
        }
    }
    entries.sort();

    let generated = format!(
        "pub static SRS_PAYLOADS: &[(&str, Option<u32>, &[u8])] = &[\n{}\n];\n",
        entries
            .iter()
            .map(|entry| format!("    {entry},"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let out = Path::new(&env::var("OUT_DIR").expect("OUT_DIR")).join("srs_payloads.rs");
    fs::write(out, generated).expect("write the payload table");
}
