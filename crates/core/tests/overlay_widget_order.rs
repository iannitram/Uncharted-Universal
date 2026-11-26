//! Guard for Issue #153: no overlay widget is evaluated in an unspecified order.
//!
//! The Windows CLANG64 build read `v` before `ImGui::Checkbox(label, &v)` ran in
//! `Store(value, v, ImGui::Checkbox(label, &v))`, so every checkbox and slider stored the old
//! value again (C++ leaves the order of argument evaluation unspecified). The fix calls the
//! widget first and stores its result. This test scans the port's own C++ in `gpu/shim` and fails
//! when a statement passes `&name` to an `ImGui::` widget and also uses `name`, or calls
//! `Store(` with an `ImGui::` widget inside it, anywhere in one expression.

use std::fs;
use std::path::{Path, PathBuf};

/// Folders of `gpu/shim` that are third-party code, not the port's.
const SKIPPED_DIRS: [&str; 5] = ["imgui", "tracy", "common", "core", "video_core"];

fn shim_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../gpu/shim")
}

/// `source` with comments and string/char literals blanked (same length, newlines kept).
fn blank_comments_and_literals(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = bytes.to_vec();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        let next = bytes.get(i + 1).copied().unwrap_or(0);
        if c == b'/' && next == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                out[i] = b' ';
                i += 1;
            }
        } else if c == b'/' && next == b'*' {
            out[i] = b' ';
            out[i + 1] = b' ';
            i += 2;
            while i < bytes.len() && !(bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/')) {
                if bytes[i] != b'\n' {
                    out[i] = b' ';
                }
                i += 1;
            }
            if i < bytes.len() {
                out[i] = b' ';
                out[i + 1] = b' ';
                i += 2;
            }
        } else if c == b'"' || c == b'\'' {
            let quote = c;
            i += 1;
            while i < bytes.len() && bytes[i] != quote && bytes[i] != b'\n' {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    out[i] = b' ';
                    i += 1;
                }
                out[i] = b' ';
                i += 1;
            }
            i += 1;
        } else {
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Whether `name` occurs in `text` as a whole identifier.
fn has_word(text: &str, name: &str) -> bool {
    let bytes = text.as_bytes();
    let mut from = 0;
    while let Some(pos) = text[from..].find(name) {
        let start = from + pos;
        let end = start + name.len();
        let before = start == 0 || !is_ident(bytes[start - 1]);
        let after = end >= bytes.len() || !is_ident(bytes[end]);
        if before && after {
            return true;
        }
        from = start + 1;
    }
    false
}

/// `(start, end)` of each `ImGui::Name( ... )` call in `statement`, the end after its `)`.
fn widget_calls(statement: &str) -> Vec<(usize, usize)> {
    let bytes = statement.as_bytes();
    let mut calls = Vec::new();
    let mut from = 0;
    while let Some(pos) = statement[from..].find("ImGui::") {
        let start = from + pos;
        let mut i = start + "ImGui::".len();
        while i < bytes.len() && is_ident(bytes[i]) {
            i += 1;
        }
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'(' {
            let mut depth = 0usize;
            let mut end = i;
            while end < bytes.len() {
                match bytes[end] {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            end += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                end += 1;
            }
            calls.push((start, end.min(bytes.len())));
            from = end.min(bytes.len());
        } else {
            from = i.max(start + 1);
        }
    }
    calls
}

/// The `&name` arguments (the variable a widget writes) in a widget call's text.
fn address_args(call: &str) -> Vec<String> {
    let bytes = call.as_bytes();
    let mut names = Vec::new();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'&' || bytes.get(i + 1) == Some(&b'&') || (i > 0 && bytes[i - 1] == b'&') {
            continue;
        }
        let mut j = i + 1;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        let start = j;
        while j < bytes.len() && (is_ident(bytes[j]) || bytes[j] == b'.') {
            j += 1;
        }
        if j > start {
            names.push(call[start..j].to_owned());
        }
    }
    names
}

/// Problems in `source`: statements where a widget and a use of its variable share one expression.
fn find_hazards(source: &str) -> Vec<String> {
    let clean = blank_comments_and_literals(source);
    let mut found = Vec::new();
    // A statement ends at `;`, or at `{` / `}` (so `if (ImGui::X(&v)) {` is a statement of its own).
    let mut line = 1;
    let mut start = 0;
    let mut start_line = 1;
    for (i, ch) in clean.char_indices() {
        if ch == '\n' {
            line += 1;
        }
        if ch == ';' || ch == '{' || ch == '}' {
            check_statement(&clean[start..i], start_line, &mut found);
            start = i + 1;
            start_line = line;
        }
    }
    check_statement(&clean[start..], start_line, &mut found);
    found
}

fn check_statement(statement: &str, line: usize, found: &mut Vec<String>) {
    let calls = widget_calls(statement);
    if calls.is_empty() {
        return;
    }
    let first_line = line
        + statement
            .chars()
            .take_while(|c| c.is_whitespace())
            .filter(|&c| c == '\n')
            .count();
    let mut flagged = false;
    // A `Store(` that has a widget inside its arguments.
    for (call_start, _) in &calls {
        if let Some(store) = statement.find("Store(") {
            let store_ok_boundary = store == 0 || !is_ident(statement.as_bytes()[store - 1]);
            if store_ok_boundary && store < *call_start {
                found.push(format!(
                    "line {first_line}: `Store(` with an ImGui widget inside it"
                ));
                flagged = true;
                break;
            }
        }
    }
    if flagged {
        return;
    }
    // A widget that writes `&name`, with `name` also used outside the widget call.
    for &(call_start, call_end) in &calls {
        for name in address_args(&statement[call_start..call_end]) {
            let outside = format!("{}{}", &statement[..call_start], &statement[call_end..]);
            let base = name.split('.').next().unwrap_or(&name);
            let used_elsewhere = has_word(&outside, base);
            // Another widget in the same expression is its own hazard only when it shares the name.
            if used_elsewhere {
                found.push(format!(
                    "line {first_line}: `{name}` is passed to an ImGui widget and used again in the same expression"
                ));
                return;
            }
        }
    }
}

fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !SKIPPED_DIRS.contains(&name) {
                collect(&path, files);
            }
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("cpp" | "h" | "hpp" | "cc")
        ) {
            files.push(path);
        }
    }
}

#[test]
fn the_detector_finds_the_old_pattern() {
    let bad = "void Checkbox(const char* label, std::atomic<bool>& value) {\n    bool v = value;\n    Store(value, v, ImGui::Checkbox(label, &v));\n}\n";
    assert_eq!(find_hazards(bad).len(), 1, "{:?}", find_hazards(bad));
    let slider =
        "    bool changed_and_v = Foo(v, ImGui::SliderFloat(label, &v, 0.0f, 1.0f, \"%.2f\"));\n";
    assert_eq!(find_hazards(slider).len(), 1);
    let combo = "    Store(s.upscaler, i, ImGui::Selectable(upscalers[i], i == upscaler));\n";
    assert_eq!(find_hazards(combo).len(), 1);
}

#[test]
fn the_detector_accepts_the_fixed_forms() {
    let good = "void Checkbox(const char* label, std::atomic<bool>& value) {\n    bool v = value;\n    // Store(value, v, ImGui::Checkbox(label, &v)) is the old, broken form\n    const bool changed = ImGui::Checkbox(label, &v);\n    Store(value, v, changed);\n}\n";
    assert!(find_hazards(good).is_empty(), "{:?}", find_hazards(good));
    let in_if = "    if (ImGui::Checkbox(T(\"Show\", \"x\"), &show_mask)) {\n        Store(s.debug_view, show_mask ? 1 : 0, true);\n    }\n";
    assert!(find_hazards(in_if).is_empty(), "{:?}", find_hazards(in_if));
    let slider_int = "    if (ImGui::SliderInt(label, &multiplier, 2, std::max(max_multiplier, 2), \"%dx\")) {\n    }\n";
    assert!(find_hazards(slider_int).is_empty());
}

#[test]
fn no_overlay_widget_is_evaluated_in_an_unspecified_order() {
    let mut files = Vec::new();
    collect(&shim_dir(), &mut files);
    assert!(
        files.iter().any(|f| f.ends_with("bbport_overlay.cpp")),
        "gpu/shim/bbport_overlay.cpp not found under {}",
        shim_dir().display()
    );
    let mut problems = Vec::new();
    for file in &files {
        let source = fs::read_to_string(file).expect("read C++ source");
        for hazard in find_hazards(&source) {
            problems.push(format!("{}: {hazard}", file.display()));
        }
    }
    assert!(
        problems.is_empty(),
        "call the widget first, then store its result (issue #153):\n{}",
        problems.join("\n")
    );
}
