use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "assets/"]
pub struct Assets;

#[derive(RustEmbed)]
#[folder = "internal/"]
pub struct Internal;

#[derive(RustEmbed)]
#[folder = "../prompts/"]
pub struct Prompts;
