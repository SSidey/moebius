use std::fs;
use std::path::Path;

#[derive(serde::Deserialize)]
pub(super) struct ValidationSchema {
    pub(super) frontmatter: FrontmatterSchema,
    pub(super) body: BodySchema,
}

#[derive(serde::Deserialize)]
pub(super) struct FrontmatterSchema {
    pub(super) required: Vec<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub(super) optional: Vec<String>,
}

#[derive(serde::Deserialize)]
pub(super) struct BodySchema {
    pub(super) required_sections: Vec<String>,
}

pub(super) fn load_validation_schema(working_dir: &Path) -> Option<ValidationSchema> {
    let path = working_dir.join("spec-schema-validation.json");
    match fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<ValidationSchema>(&content) {
            Ok(schema) => Some(schema),
            Err(e) => {
                eprintln!(
                    "[moeb] warning: spec-schema-validation.json is malformed ({}); \
                     falling back to built-in validation rules.",
                    e
                );
                None
            }
        },
        Err(_) => {
            eprintln!(
                "[moeb] warning: spec-schema-validation.json not found in {:?}; \
                 falling back to built-in validation rules.",
                working_dir
            );
            None
        }
    }
}
