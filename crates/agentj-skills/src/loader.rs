use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Priority order (design §5): .aj > .agents > .claude > builtin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SkillPriority {
    Builtin,
    Claude,
    Agents,
    Aj,
}

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub path: PathBuf,
    pub description: String,
    pub priority: SkillPriority,
}

/// Scan `base` for `.aj/skills`, `.agents/skills`, `.claude/skills`; each skill is a
/// subdirectory containing SKILL.md. Same-name skills: highest priority wins (map merge
/// with max). Different-name skills: all loaded.
pub fn load_skills(base: &Path) -> Result<Vec<Skill>, Box<dyn std::error::Error>> {
    let mut merged: HashMap<String, Skill> = HashMap::new();
    for (subdir, priority) in [
        (".aj/skills", SkillPriority::Aj),
        (".agents/skills", SkillPriority::Agents),
        (".claude/skills", SkillPriority::Claude),
    ] {
        let dir = base.join(subdir);
        if !dir.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let skill_dir = entry.path();
            if !skill_dir.is_dir() {
                continue;
            }
            let md_path = skill_dir.join("SKILL.md");
            if !md_path.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let description = match std::fs::read_to_string(&md_path) {
                Ok(desc) => desc,
                Err(err) => {
                    tracing::warn!(skill = %name, path = %md_path.display(), error = %err, "skipping skill: cannot read SKILL.md");
                    continue;
                }
            };
            let candidate = Skill {
                name: name.clone(),
                path: skill_dir,
                description,
                priority,
            };
            match merged.get(&name) {
                Some(existing) if existing.priority >= candidate.priority => {}
                _ => {
                    merged.insert(name, candidate);
                }
            }
        }
    }
    Ok(merged.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_skill(dir: &std::path::Path, name: &str, content: &str) {
        let skill_dir = dir.join(name);
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), content).unwrap();
    }

    #[test]
    fn same_name_higher_priority_wins() {
        let dir = tempfile::TempDir::new().unwrap();
        write_skill(&dir.path().join(".aj/skills"), "echo", "AJ VERSION");
        write_skill(&dir.path().join(".claude/skills"), "echo", "CLAUDE VERSION");
        let skills = load_skills(dir.path()).unwrap();
        let echo = skills.iter().find(|s| s.name == "echo").unwrap();
        assert_eq!(
            echo.description, "AJ VERSION",
            ".aj wins over .claude for same name"
        );
        assert_eq!(echo.priority, SkillPriority::Aj);
    }

    #[test]
    fn different_names_all_loaded() {
        let dir = tempfile::TempDir::new().unwrap();
        write_skill(&dir.path().join(".aj/skills"), "ajskill", "AJ");
        write_skill(&dir.path().join(".claude/skills"), "claudeskill", "CLAUDE");
        let skills = load_skills(dir.path()).unwrap();
        assert_eq!(skills.len(), 2);
        assert!(skills.iter().any(|s| s.name == "ajskill"));
        assert!(skills.iter().any(|s| s.name == "claudeskill"));
    }

    #[test]
    fn empty_dir_loads_nothing() {
        let dir = tempfile::TempDir::new().unwrap();
        let skills = load_skills(dir.path()).unwrap();
        assert!(skills.is_empty());
    }

    #[test]
    fn non_utf8_skill_md_is_skipped_not_fatal() {
        let dir = tempfile::TempDir::new().unwrap();
        write_skill(&dir.path().join(".aj/skills"), "good", "GOOD");
        // SKILL.md with invalid UTF-8 bytes (0xFF is not valid in UTF-8).
        let bad_dir = dir.path().join(".aj/skills/bad");
        std::fs::create_dir_all(&bad_dir).unwrap();
        std::fs::write(bad_dir.join("SKILL.md"), [0xFF, 0xFE, 0x00]).unwrap();

        let skills = load_skills(dir.path()).unwrap();
        assert_eq!(skills.len(), 1, "bad skill skipped, good skill still loaded");
        assert_eq!(skills[0].name, "good");
        assert_eq!(skills[0].description, "GOOD");
    }
}
