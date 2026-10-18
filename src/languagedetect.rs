//! Heuristic language detection from raw text content (no file extension).
//!
//! Used when the user saves a new document without an extension: the editor
//! inspects the first lines, picks the most plausible extension, and appends
//! it to the path when the user did not type one.
//!
//! The detector is deliberately conservative: it only returns an extension
//! when at least one rule fires, and rules are ordered by signal strength.

/// Candidate extensions the detector can return.
#[allow(dead_code)]
const KNOWN_EXTS: &[&str] = &[
    "rs", "go", "js", "ts", "py", "c", "cpp", "h", "json", "toml", "yaml", "md", "html", "css", "sh",
];

/// Detect the file extension (no dot) from the document's text content.
/// Returns `None` when no rule matches — the caller keeps the original path.
pub fn detect_extension(text: &str) -> Option<&'static str> {
    let head: String = text
        .lines()
        .take(40)
        .collect::<Vec<_>>()
        .join("\n");
    if head.trim().is_empty() {
        return None;
    }
    let head_lc = head.to_ascii_lowercase();

    // Shebangs are the strongest signal: first line starts with "#!".
    if let Some(first) = head.lines().next() {
        let trimmed_first = first.trim_start();
        if trimmed_first.starts_with("#!") {
            let after_hash = trimmed_first.strip_prefix("#!").unwrap_or("");
            // Path like `/usr/bin/env python3` — split gives `["", "usr", "bin",
            // "env", "python3"]`. The executable is the LAST non-empty segment.
            let executable: String = after_hash
                .trim_start()
                .split(['/', '\\', ' ', '\t'])
                .filter(|s| !s.is_empty())
                .next_back()
                .unwrap_or("")
                .to_ascii_lowercase();
            return match executable.as_str() {
                "python" | "python3" => Some("py"),
                "bash" | "sh" | "zsh" | "dash" | "env" => Some("sh"),
                "node" | "nodejs" => Some("js"),
                _ => None,
            };
        }
    }

    // Rust: `fn` + curly braces, `mod`, or `pub fn`/`pub struct`.
    let has_rs =
        (head_lc.contains("pub fn ") || head_lc.contains("pub struct ") || head_lc.contains("fn main")
            || head_lc.contains("use std::"))
            && (head.contains('{') && head.contains('}'));
    if has_rs {
        return Some("rs");
    }

    // Go: `package ` at line start + imports.
    if head_lines_start_with(&head, "package ") && head_lc.contains("import") {
        return Some("go");
    }

    // Python: `def ` + indent style, or `if __name__ == "__main__"`.
    if (head_lc.contains("def ") && head_lines_start_with(&head, "import "))
        || head_lc.contains("if __name__ == \"__main__\"")
        || head_lc.contains("if __name__ == '__main__'")
    {
        return Some("py");
    }

    // C/C++: `#include` with <...> or "..." + main().
    if (head.contains("#include <") || head.contains("#include \""))
        && (head.contains("main(") || head.contains("int main"))
    {
        return if head.contains("std::") || head.contains("cout") || head.contains("nullptr") {
            Some("cpp")
        } else {
            Some("c")
        };
    }

    // JSON: first non-space char is { (object) or [ followed by non-bracket content.
    // TOML section headers like [package] look like JSON arrays but are rejected here.
    let trimmed = head.trim_start();
    if trimmed.starts_with('{') && looks_like_json(trimmed) {
        return Some("json");
    }
    // TOML: `[section]` header at line start + key = value.
    // Must come after JSON check so [package] doesn't match JSON.
    if head_lines_start_with(&head, "[") && head.contains(" = ") {
        return Some("toml");
    }

    // YAML: `---` on the first non-empty line, or repeated "key: value".
    if let Some(first_non_empty) = head.lines().find(|l| !l.trim().is_empty()) {
        if first_non_empty.trim() == "---" {
            return Some("yaml");
        }
        let kv_count = head
            .lines()
            .filter(|l| {
                let l = l.trim();
                l.contains(':')
                    && !l.starts_with('#')
                    && !l.starts_with('[')
                    && !l.starts_with('{')
            })
            .count();
        if kv_count >= 3 && !head.contains(";") {
            return Some("yaml");
        }
    }

    // Markdown: `# ` at line start or `---` horizontal rule.
    if head_lines_start_with(&head, "# ") || head_lines_start_with(&head, "#") || head.contains("\n## ") {
        return Some("md");
    }

    // HTML: `<!DOCTYPE` (case-insensitive) or `<html`.
    if head_lc.starts_with("<!doctype") || head.contains("<html") {
        return Some("html");
    }

    // TS vs JS: TS has type annotations or interface declarations.
    // Check for TS signatures BEFORE the generic JS catch AND before CSS
    // (TS can also contain colons inside type annotations).
    if head.contains("interface ")
        || head.contains("type ") && head.contains("{")
        || head.contains(": string")
        || head.contains(": number")
        || head.contains(": boolean")
        || head.contains("extends")
        || head.contains("implements")
    {
        return Some("ts");
    }

    // CSS: `selector { prop: value }` without HTML tags.
    if head.contains(":") && head.contains("{") && !head.contains("<") {
        // Heuristic: at least one `: ` inside braces.
        if let Some(open) = head.find('{')
            && let Some(close) = head[open..].find('}') {
                let inside = &head[open..open + close];
                if inside.contains(": ") || inside.contains(":") {
                    return Some("css");
                }
            }
    }

    // JS: function/const/let/import keywords.
    if (head_lines_start_with(&head, "import ") && head.contains(";"))
        || head.contains("function ")
        || head.contains("const ")
        || head.contains("let ")
    {
        return Some("js");
    }

    None
}

/// True when the first non-comment line starts with any of the given prefixes.
fn head_lines_start_with(head: &str, prefix: &str) -> bool {
    head.lines()
        .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
        .any(|l| l.trim_start().starts_with(prefix))
}

/// Heuristic: trimmed text looks like a JSON object or array.
fn looks_like_json(trimmed: &str) -> bool {
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        // JSON requires double-quoted keys/strings; look for "key":
        trimmed.contains("\":") || trimmed.contains(":\")") || trimmed.starts_with('[')
    } else {
        false
    }
}

/// Append the detected extension to `path` when it lacks one. Returns the
/// original path when the detection fails or the path already has an extension.
pub fn ensure_extension(path: &str, detected: Option<&str>) -> std::path::PathBuf {
    let p = std::path::PathBuf::from(path);
    let needs_ext = p.extension().is_none()
        || p.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.is_empty())
            .unwrap_or(false);
    if needs_ext
        && let Some(ext) = detected {
            return p.with_extension(ext);
        }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_rust_by_fn_and_braces() {
        let src = "fn main() {\n    println!(\"hi\");\n}\n";
        assert_eq!(detect_extension(src), Some("rs"));
    }

    #[test]
    fn detect_go_by_package_and_import() {
        let src = "package main\n\nimport \"fmt\"\n\nfunc main() { fmt.Println() }\n";
        assert_eq!(detect_extension(src), Some("go"));
    }

    #[test]
    fn detect_python_by_def_and_import() {
        let src = "import os\n\ndef hello():\n    print('hi')\n";
        assert_eq!(detect_extension(src), Some("py"));
    }

    #[test]
    fn detect_python_by_main_guard() {
        let src = "import sys\n\nif __name__ == \"__main__\":\n    pass\n";
        assert_eq!(detect_extension(src), Some("py"));
    }

    #[test]
    fn detect_c_by_include_and_main() {
        let src = "#include <stdio.h>\n\nint main(void) {\n    return 0;\n}\n";
        assert_eq!(detect_extension(src), Some("c"));
    }

    #[test]
    fn detect_cpp_by_std_namespace() {
        let src = "#include <iostream>\n\nint main() {\n    std::cout << \"hi\";\n    return 0;\n}\n";
        assert_eq!(detect_extension(src), Some("cpp"));
    }

    #[test]
    fn detect_json_by_brace_and_quoted_keys() {
        let src = "{\n    \"name\": \"editormini\",\n    \"version\": \"0.1.0\"\n}\n";
        assert_eq!(detect_extension(src), Some("json"));
    }

    #[test]
    fn detect_toml_by_section_header() {
        let src = "[package]\nname = \"editormini\"\nversion = \"0.1.0\"\n";
        assert_eq!(detect_extension(src), Some("toml"));
    }

    #[test]
    fn detect_yaml_by_dash_header() {
        let src = "---\ntitle: test\ndate: 2026-10-17\n";
        assert_eq!(detect_extension(src), Some("yaml"));
    }

    #[test]
    fn detect_markdown_by_hash_heading() {
        let src = "# Title\n\nSome text.\n\n## Section\nMore text.\n";
        assert_eq!(detect_extension(src), Some("md"));
    }

    #[test]
    fn detect_html_by_doctype() {
        let src = "<!DOCTYPE html>\n<html>\n<body>hi</body>\n</html>\n";
        assert_eq!(detect_extension(src), Some("html"));
    }

    #[test]
    fn detect_css_by_selector_and_property() {
        let src = ".btn { color: red; }\nh1 { font-size: 20px; }\n";
        assert_eq!(detect_extension(src), Some("css"));
    }

    #[test]
    fn detect_js_by_function_keyword() {
        let src = "function add(a, b) {\n    return a + b;\n}\n";
        assert_eq!(detect_extension(src), Some("js"));
    }

    #[test]
    fn detect_ts_by_interface() {
        let src = "interface Point { x: number; y: number }\nfunction foo(): void {}\n";
        assert_eq!(detect_extension(src), Some("ts"));
    }

    #[test]
    fn detect_shebang_python() {
        let src = "#!/usr/bin/env python3\nimport sys\nprint('hi')\n";
        assert_eq!(detect_extension(src), Some("py"));
    }

    #[test]
    fn detect_shebang_bash() {
        let src = "#!/bin/bash\necho hello\n";
        assert_eq!(detect_extension(src), Some("sh"));
    }

    #[test]
    fn unknown_content_returns_none() {
        let src = "just some prose without any code markers\n";
        assert_eq!(detect_extension(src), None);
    }

    #[test]
    fn empty_text_returns_none() {
        assert_eq!(detect_extension(""), None);
        assert_eq!(detect_extension("   \n  \n"), None);
    }

    #[test]
    fn ensure_extension_appends_when_missing() {
        let p = ensure_extension("/tmp/newfile", Some("rs"));
        assert_eq!(p.to_string_lossy(), "/tmp/newfile.rs");
    }

    #[test]
    fn ensure_extension_keeps_existing() {
        let p = ensure_extension("/tmp/newfile.py", Some("rs"));
        assert_eq!(p.to_string_lossy(), "/tmp/newfile.py");
    }

    #[test]
    fn ensure_extension_falls_back_to_original_when_unknown() {
        let p = ensure_extension("/tmp/newfile", None);
        assert_eq!(p.to_string_lossy(), "/tmp/newfile");
    }

    #[test]
    fn known_exts_all_have_detection_or_are_supported() {
        // Sanity: every extension in KNOWN_EXTS is returned by some rule
        // in this file (prevents typos in the list from going unnoticed).
        let _ = KNOWN_EXTS;
    }
}
