use agentj_protocol::schema::generate_schemas;
use std::fs;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    fs::create_dir_all(&dir)?;
    let schemas = generate_schemas();
    let count = schemas.len();
    for (name, schema) in schemas {
        let path = dir.join(format!("{name}.json"));
        fs::write(&path, serde_json::to_string_pretty(&schema)? + "\n")?;
    }
    println!("wrote {count} schemas to {}", dir.display());
    Ok(())
}
