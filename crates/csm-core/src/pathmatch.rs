//! Directory comparison helpers shared by the app's window-matching and the
//! state machine's parent/child de-duplication. Single source of truth so the
//! SAME normalization is used everywhere (mismatched normalization between call
//! sites would make a dir match in one place and not another).
//!
//! Windows semantics: case-insensitive, `\`-separated, no trailing separator.

/// Normalize a directory for comparison: trimmed, lowercased, `/`→`\`, no
/// trailing slash.
pub fn normalize_dir(s: &str) -> String {
    s.trim()
        .to_lowercase()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_string()
}

/// True if two dirs are the same, or one is an ancestor of the other (after
/// normalization). Empty dirs never overlap.
pub fn dir_overlap(a: &str, b: &str) -> bool {
    let (a, b) = (normalize_dir(a), normalize_dir(b));
    !a.is_empty()
        && !b.is_empty()
        && (a == b || a.starts_with(&format!("{b}\\")) || b.starts_with(&format!("{a}\\")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_is_case_and_separator_insensitive() {
        assert_eq!(normalize_dir("D:/Foo/Bar/"), "d:\\foo\\bar");
        assert_eq!(normalize_dir("  D:\\Foo\\Bar\\\\ "), "d:\\foo\\bar");
        assert_eq!(normalize_dir("/home/me/Proj"), "\\home\\me\\proj");
        assert_eq!(normalize_dir(""), "");
    }

    #[test]
    fn overlap_matches_equal_and_ancestor_descendant() {
        assert!(dir_overlap("D:\\a\\b", "d:/a/b")); // equal (normalized)
        assert!(dir_overlap("D:\\a", "D:\\a\\b")); // ancestor
        assert!(dir_overlap("D:\\a\\b\\c", "D:\\a")); // descendant
    }

    #[test]
    fn overlap_rejects_siblings_prefixes_and_empty() {
        assert!(!dir_overlap("D:\\a\\b", "D:\\a\\c")); // siblings
        assert!(!dir_overlap("D:\\app", "D:\\app2")); // shared prefix, not a path boundary
        assert!(!dir_overlap("", "D:\\a")); // empty
        assert!(!dir_overlap("D:\\a", "")); // empty
    }
}
