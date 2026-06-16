//! Файловые скилы — markdown-инструкции с frontmatter, которые агент
//! подхватывает по триггеру и инжектит в system-промпт. Идея из cozby-claw:
//! расширяемость поведения без перекомпиляции.
//!
//! Формат `.skills/<name>.md`:
//! ```text
//! ---
//! name: incident-response
//! trigger: дежурство, incident, on-call, runbook
//! when: пользователь заводит дежурство/инцидент
//! ---
//! # Тело инструкции в markdown…
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    /// Слова/фразы-триггеры (lowercase). Совпадение с raw-входом активирует скил.
    pub trigger: Vec<String>,
    /// Когда применять (человекочитаемое; идёт в промпт как подсказка).
    pub when: String,
    /// Тело инструкции (markdown без frontmatter).
    pub body: String,
}

/// Корни поиска скилов: `<cwd>/.skills` и `$HOME/.cozby/skills`.
pub fn default_skill_roots(cwd: &Path) -> Vec<PathBuf> {
    let mut roots = vec![cwd.join(".skills")];
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join(".cozby").join("skills"));
    }
    roots
}

/// Загружает все `*.md` скилы из переданных корней. Несуществующие корни и
/// нечитаемые файлы тихо пропускаются (не падаем).
pub fn discover_skills(roots: &[PathBuf]) -> Vec<Skill> {
    let mut out = Vec::new();
    let mut seen_names = std::collections::BTreeSet::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let Ok(contents) = std::fs::read_to_string(&path) else {
                continue;
            };
            if let Some(skill) = parse_skill(&contents, &path) {
                // первый корень побеждает (cwd важнее HOME)
                if seen_names.insert(skill.name.clone()) {
                    out.push(skill);
                }
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Парсит один скил из содержимого файла. `path` — для дефолтного имени.
pub fn parse_skill(contents: &str, path: &Path) -> Option<Skill> {
    let (fm, body) = parse_frontmatter(contents);
    let name = fm
        .get("name")
        .cloned()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_string)
        })?;
    let trigger = fm
        .get("trigger")
        .map(|s| {
            s.split(',')
                .map(|t| t.trim().to_lowercase())
                .filter(|t| !t.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let when = fm.get("when").cloned().unwrap_or_default();
    Some(Skill {
        name,
        trigger,
        when,
        body: body.trim().to_string(),
    })
}

/// Выбирает скилы, чьи триггер-слова встречаются в `raw` (по подстроке,
/// case-insensitive). Возвращает в порядке количества совпавших триггеров.
pub fn select_skills<'a>(skills: &'a [Skill], raw: &str) -> Vec<&'a Skill> {
    let haystack = raw.to_lowercase();
    let mut scored: Vec<(usize, &Skill)> = skills
        .iter()
        .filter_map(|s| {
            let hits = s
                .trigger
                .iter()
                .filter(|t| haystack.contains(t.as_str()))
                .count();
            (hits > 0).then_some((hits, s))
        })
        .collect();
    scored.sort_by_key(|(hits, _)| std::cmp::Reverse(*hits));
    scored.into_iter().map(|(_, s)| s).collect()
}

/// Минимальный YAML-frontmatter парсер (плоские `key: value`). Возвращает
/// (map, body). Если frontmatter нет — map пустой, body = весь текст.
pub fn parse_frontmatter(contents: &str) -> (HashMap<String, String>, String) {
    let mut map = HashMap::new();
    let mut lines = contents.lines();
    if lines.next().map(str::trim) != Some("---") {
        return (map, contents.to_string());
    }
    let mut body_lines: Vec<&str> = Vec::new();
    let mut in_frontmatter = true;
    for line in lines {
        if in_frontmatter {
            if line.trim() == "---" {
                in_frontmatter = false;
                continue;
            }
            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim().to_lowercase();
                let value = unquote(value.trim());
                if !key.is_empty() && !value.is_empty() {
                    map.insert(key, value);
                }
            }
        } else {
            body_lines.push(line);
        }
    }
    (map, body_lines.join("\n"))
}

fn unquote(value: &str) -> String {
    value
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|t| t.strip_suffix('\'')))
        .unwrap_or(value)
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parses_frontmatter_and_body() {
        let md = "---\nname: incident-response\ntrigger: дежурство, incident, runbook\nwhen: при заведении дежурства\n---\n# Тело\nшаг 1\n";
        let skill = parse_skill(md, &PathBuf::from("x.md")).unwrap();
        assert_eq!(skill.name, "incident-response");
        assert_eq!(skill.trigger, vec!["дежурство", "incident", "runbook"]);
        assert_eq!(skill.when, "при заведении дежурства");
        assert!(skill.body.starts_with("# Тело"));
    }

    #[test]
    fn name_falls_back_to_filename() {
        let skill = parse_skill("no frontmatter here", &PathBuf::from("oncall-setup.md")).unwrap();
        assert_eq!(skill.name, "oncall-setup");
        assert!(skill.trigger.is_empty());
    }

    #[test]
    fn selects_by_trigger_match() {
        let skills = vec![
            Skill {
                name: "incident".into(),
                trigger: vec!["дежурство".into(), "incident".into()],
                when: String::new(),
                body: String::new(),
            },
            Skill {
                name: "shopping".into(),
                trigger: vec!["купить".into()],
                when: String::new(),
                body: String::new(),
            },
        ];
        let selected = select_skills(&skills, "Заступаю на ДЕЖУРСТВО по billing");
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].name, "incident");
    }
}
