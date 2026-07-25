use std::{env, fs, path::Path};

/// Collects whatever SRS and Lagrange payloads have been exported into
/// `assets/precomputed/` and emits a table of them. The payloads are committed
/// so that a fresh clone builds an app that starts fast, but they stay
/// optional: when the directory is empty the table is empty too and the app
/// simply recomputes them on first use.
///
/// Names follow `srs-<curve>.bin` and `lagrange-<curve>-<domain_log2>.bin`,
/// as written by the `export_srs_payloads` test.
fn main() {
    emit_dependency_revisions();

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

/// Exposes the git revisions the proving stack was locked to, so a build on a
/// phone can be told apart from another one: the app reports them next to the
/// backend versions. `Cargo.lock` is the only source that knows them, since the
/// manifest only names branches.
fn emit_dependency_revisions() {
    println!("cargo:rerun-if-changed=Cargo.lock");
    let lock = fs::read_to_string("Cargo.lock").unwrap_or_default();

    for (repository, variable) in [
        ("proof-systems", "BUILD_PROOF_SYSTEMS_REV"),
        ("mina-rust", "BUILD_MINA_RUST_REV"),
    ] {
        let revision = lock
            .lines()
            .filter_map(|line| line.trim().strip_prefix("source = \"git+"))
            .find(|source| source.contains(repository))
            .and_then(|source| source.rsplit_once('#'))
            .map(|(_, revision)| revision.trim_end_matches('"'))
            .map(|revision| revision.chars().take(8).collect::<String>())
            .unwrap_or_else(|| "unknown".to_owned());
        println!("cargo:rustc-env={variable}={revision}");
    }
}
