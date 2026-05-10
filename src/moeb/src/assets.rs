use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "assets/"]
pub struct Assets;

#[derive(RustEmbed)]
#[folder = "../prompts/"]
pub struct Prompts;
