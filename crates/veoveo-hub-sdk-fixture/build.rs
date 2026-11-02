//! Observatory's skills, embedded from `skills/` as a hosted server embeds its own.
fn main() {
    veoveo_skills::build::embed_skills("skills").expect("Observatory's skills");
}
