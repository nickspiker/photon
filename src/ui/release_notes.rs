//! Release notes, compiled in (RELEASE_NOTES.md at the repo root). Shipped versions are named by their minor number alone — v87 — and the file's top section is always `## Upcoming`, which a deploy renames to the version it ships as. The app shows the section for the version it runs; a dev build also shows Upcoming, since that is what it is.

const NOTES: &str = include_str!("../../RELEASE_NOTES.md");

/// The bullet lines under the heading `## <name>` — `"v87"` or `"Upcoming"` — in file order; empty when the section is absent.
pub fn section(name: &str) -> Vec<String> {
    section_in(NOTES, name)
}

fn section_in(text: &str, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            inside = h.trim() == name;
            continue;
        }
        if inside {
            if let Some(item) = line.trim_start().strip_prefix("- ") {
                out.push(item.trim().to_string());
            }
        }
    }
    out
}

/// Every version section's name, newest first (the file order), Upcoming excluded.
pub fn shipped_versions() -> Vec<String> {
    NOTES
        .lines()
        .filter_map(|l| l.strip_prefix("## "))
        .map(|h| h.trim().to_string())
        .filter(|h| h != "Upcoming")
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# Release notes\n\nprose\n\n## Upcoming\n\n- soon\n\n## v87\n\n- one\n- two\n\n## v86\n\n- old\n";

    #[test]
    fn sections_are_read_by_heading_and_bullets_only() {
        assert_eq!(section_in(SAMPLE, "v87"), vec!["one", "two"]);
        assert_eq!(section_in(SAMPLE, "Upcoming"), vec!["soon"]);
        assert_eq!(section_in(SAMPLE, "v1"), Vec::<String>::new());
    }

    /// The real file keeps the contract the deploy relies on: an Upcoming section at the top, and every other heading a bare minor version.
    #[test]
    fn the_shipped_file_keeps_the_contract() {
        let heads: Vec<&str> = NOTES.lines().filter_map(|l| l.strip_prefix("## ")).map(str::trim).collect();
        assert_eq!(heads.first().copied(), Some("Upcoming"), "the top section is always Upcoming");
        for h in heads.iter().skip(1) {
            assert!(h.starts_with('v') && h[1..].chars().all(|c| c.is_ascii_digit()), "a shipped section is a bare minor version, got {h}");
        }
        assert!(!shipped_versions().is_empty());
    }
}
