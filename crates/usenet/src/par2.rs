//! PAR2 verify and repair (via the pure-Rust `rust-par2` crate).

use std::path::Path;

use anyhow::{Context, bail};

/// Verify the set described by `index` (its recovery volumes are found in
/// the same folder) and repair whatever is damaged or missing. Returns a
/// one-line summary.
pub fn repair(index: &Path) -> anyhow::Result<String> {
    let dir = index.parent().context("PAR2 file has no folder")?;
    let set = rust_par2::parse(index).map_err(|e| anyhow::anyhow!("couldn't read {}: {e}", index.display()))?;
    let verified = rust_par2::verify(&set, dir);
    if verified.all_correct() {
        return Ok("Verified: every file is intact".into());
    }
    let needed = verified.blocks_needed();
    if !verified.repair_possible {
        bail!(
            "Too much is missing to repair: {needed} blocks are damaged and only {} recovery blocks are available",
            verified.recovery_blocks_available
        );
    }
    let result = rust_par2::repair_from_verify(&set, dir, &verified).map_err(|e| anyhow::anyhow!("{e}"))?;
    if !result.success {
        bail!("Repair didn't succeed");
    }
    Ok(format!(
        "Repaired {} {} in {} {}",
        result.blocks_repaired,
        if result.blocks_repaired == 1 { "block" } else { "blocks" },
        result.files_repaired,
        if result.files_repaired == 1 { "file" } else { "files" }
    ))
}
