use std::path::Path;

/// Resolves and returns the content of the named skill file.
///
/// Resolution order:
///   1. {moeb_dir}/skills/{name}.skill.md  (project-local override)
///   2. Binary-bundled asset skills/{name}.skill.md
///   3. Empty string with a stderr warning
pub fn load_skill(moeb_dir: &Path, name: &str) -> String {
    // 1. Project-local override
    let local_path = moeb_dir.join("skills").join(format!("{}.skill.md", name));
    if let Ok(content) = std::fs::read_to_string(&local_path) {
        return content;
    }

    // 2. Bundled binary asset
    let asset_key = format!("skills/{}.skill.md", name);
    if let Some(asset) = crate::assets::Assets::get(&asset_key) {
        if let Ok(content) = std::str::from_utf8(asset.data.as_ref()) {
            return content.to_string();
        }
    }

    // 3. Fallback
    eprintln!(
        "moeb: warning: skill '{}' not found in .moeb/skills/ or binary assets; \
         workflow section will be empty.",
        name
    );
    String::new()
}

/// Extracts the value of the `skill:` key from a spec's YAML frontmatter.
/// Returns None if the field is absent or the frontmatter cannot be parsed.
pub fn extract_skill_name(spec_content: &str) -> Option<String> {
    let body = spec_content.strip_prefix("---\n")?;
    let end = body.find("\n---")?;
    let yaml_str = &body[..end];
    for line in yaml_str.lines() {
        if let Some(val) = line.strip_prefix("skill:") {
            let name = val.trim().to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

/// Resolves and returns the content of the named role file.
///
/// Resolution order:
///   1. {moeb_dir}/roles/{name}.role.md  (project-local override)
///   2. Binary-bundled asset roles/{name}.role.md
///   3. Empty string with a stderr warning
pub fn load_role(moeb_dir: &Path, name: &str) -> String {
    let local_path = moeb_dir.join("roles").join(format!("{}.role.md", name));
    if let Ok(content) = std::fs::read_to_string(&local_path) {
        return content;
    }

    let asset_key = format!("roles/{}.role.md", name);
    if let Some(asset) = crate::assets::Assets::get(&asset_key) {
        if let Ok(content) = std::str::from_utf8(asset.data.as_ref()) {
            return content.to_string();
        }
    }

    eprintln!(
        "moeb: warning: role '{}' not found in .moeb/roles/ or binary assets; \
         role section will be empty.",
        name
    );
    String::new()
}

/// Extracts the value of the `role:` key from a spec's YAML frontmatter.
/// Returns None if the field is absent or the frontmatter cannot be parsed.
pub fn extract_role_name(spec_content: &str) -> Option<String> {
    let body = spec_content.strip_prefix("---\n")?;
    let end = body.find("\n---")?;
    let yaml_str = &body[..end];
    for line in yaml_str.lines() {
        if let Some(val) = line.strip_prefix("role:") {
            let name = val.trim().to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_skill_name_returns_some_when_present() {
        let spec = "---\ndomain: moeb\nslug: test\nstatus: active\nskill: my-skill\n---\n# Title\n";
        assert_eq!(extract_skill_name(spec), Some("my-skill".to_string()));
    }

    #[test]
    fn extract_skill_name_returns_none_when_absent() {
        let spec = "---\ndomain: moeb\nslug: test\nstatus: active\n---\n# Title\n";
        assert_eq!(extract_skill_name(spec), None);
    }

    #[test]
    fn extract_skill_name_returns_none_on_invalid_yaml() {
        let spec = "---\n: : : invalid yaml\n---\n# Title\n";
        assert_eq!(extract_skill_name(spec), None);
    }

    #[test]
    fn extract_role_name_returns_some_when_present() {
        let spec = "---\ndomain: moeb\nslug: test\nstatus: active\nrole: my-role\n---\n# Title\n";
        assert_eq!(extract_role_name(spec), Some("my-role".to_string()));
    }

    #[test]
    fn extract_role_name_returns_none_when_absent() {
        let spec = "---\ndomain: moeb\nslug: test\nstatus: active\n---\n# Title\n";
        assert_eq!(extract_role_name(spec), None);
    }
}
