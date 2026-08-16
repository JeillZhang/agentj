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
        assert_eq!(echo.description, "AJ VERSION", ".aj wins over .claude for same name");
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
}
