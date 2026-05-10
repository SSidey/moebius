use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::assets::Assets;
use crate::config::{moeb_dir, MoebConfig};

pub fn run() -> Result<()> {
    let moeb = moeb_dir();

    if moeb.exists() {
        anyhow::bail!("Already initialised. Run `moeb init --reinit` to reinitialise.");
    }

    fs::create_dir_all(&moeb).context("Failed to create .moeb/")?;

    move_or_extract("README.md")?;
    move_or_extract("spec-schema.yaml")?;

    let harness_src = Path::new("harness");
    let harness_dst = moeb.join("harness");
    if harness_src.exists() {
        fs::rename(harness_src, &harness_dst)
            .with_context(|| format!("Failed to move harness/ to {}", harness_dst.display()))?;
    } else {
        fs::create_dir_all(&harness_dst).context("Failed to create .moeb/harness/")?;
    }

    MoebConfig::default().save()?;
    ensure_gitignore()?;

    println!("Moeb initialised. Run `moeb use <adapter>` to configure an AI provider.");
    Ok(())
}

fn move_or_extract(name: &str) -> Result<()> {
    let src = Path::new(name);
    let dst = moeb_dir().join(name);

    if src.exists() {
        fs::rename(src, &dst)
            .with_context(|| format!("Failed to move {} into .moeb/", name))?;
    } else {
        let asset = Assets::get(name)
            .with_context(|| format!("Embedded asset '{}' not found in binary", name))?;
        fs::write(&dst, asset.data.as_ref())
            .with_context(|| format!("Failed to write .moeb/{}", name))?;
    }
    Ok(())
}

fn ensure_gitignore() -> Result<()> {
    let path = Path::new(".gitignore");
    let entry = ".moeb/.secrets";

    if path.exists() {
        let content = fs::read_to_string(path).context("Failed to read .gitignore")?;
        if content.lines().any(|l| l.trim() == entry) {
            return Ok(());
        }
        let mut content = content;
        if !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(entry);
        content.push('\n');
        fs::write(path, content).context("Failed to update .gitignore")?;
    } else {
        fs::write(path, format!("{}\n", entry)).context("Failed to create .gitignore")?;
    }
    Ok(())
}
