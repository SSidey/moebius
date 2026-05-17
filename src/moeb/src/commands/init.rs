use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::assets::Assets;
use crate::config::moeb_dir;

pub fn run() -> Result<()> {
    let moeb = moeb_dir();

    if moeb.exists() {
        anyhow::bail!("Already initialised. Run `moeb init --reinit` to reinitialise.");
    }

    fs::create_dir_all(&moeb).context("Failed to create .moeb/")?;

    move_or_extract("README.md")?;

    let rubrics_dst = moeb.join("rubrics");
    fs::create_dir_all(&rubrics_dst).context("Failed to create .moeb/rubrics/")?;
    fs::write(rubrics_dst.join("global.rubrics.md"), b"")
        .context("Failed to create .moeb/rubrics/global.rubrics.md")?;
    fs::write(rubrics_dst.join("rubrics.catalogue.md"), b"")
        .context("Failed to create .moeb/rubrics/rubrics.catalogue.md")?;

    let specs_src = Path::new("specifications");
    let specs_dst = moeb.join("specifications");
    if specs_src.exists() {
        fs::rename(specs_src, &specs_dst)
            .with_context(|| format!("Failed to move specifications/ to {}", specs_dst.display()))?;
    } else {
        fs::create_dir_all(&specs_dst).context("Failed to create .moeb/specifications/")?;
    }

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
