//! Open the project or a touched file in the user's editor.
//!
//! macOS launches apps by name through `open -a`, which works when the app
//! was started from Finder with a minimal `PATH`. Linux and Windows use the
//! editors' command-line launchers. Every candidate is tried in order.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::actions::TerminalLaunch as Launch;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "command")]
pub enum EditorChoice {
    /// The OS default app for the file, or the file manager for folders.
    #[default]
    System,
    VsCode,
    Cursor,
    Zed,
    /// The first installed JetBrains IDE (PhpStorm, IntelliJ IDEA, WebStorm, …).
    JetBrains,
    /// A command; `{path}` is replaced by the quoted file or folder.
    Custom(String),
}

/// JetBrains apps in the order they are tried. App name on macOS, launcher elsewhere.
const JETBRAINS: &[(&str, &str)] = &[
    ("PhpStorm", "phpstorm"),
    ("IntelliJ IDEA", "idea"),
    ("IntelliJ IDEA Ultimate", "idea"),
    ("IntelliJ IDEA CE", "idea"),
    ("WebStorm", "webstorm"),
    ("GoLand", "goland"),
    ("PyCharm", "pycharm"),
    ("PyCharm CE", "pycharm"),
    ("RustRover", "rustrover"),
    ("CLion", "clion"),
    ("RubyMine", "rubymine"),
];

fn quote_sh(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn mac_app(app: &str, path: &str) -> Launch {
    Launch {
        program: "open".into(),
        args: vec!["-a".into(), app.into(), path.into()],
        cwd: None,
        // `open -a` exits non-zero when the app is not installed.
        wait: true,
    }
}

fn cli(os: &str, program: &str, path: &str) -> Launch {
    if os == "windows" {
        // Editor launchers are .cmd shims on Windows; run them through cmd.
        Launch {
            program: "cmd.exe".into(),
            args: vec!["/c".into(), program.into(), path.into()],
            cwd: None,
            wait: true,
        }
    } else {
        Launch {
            program: program.into(),
            args: vec![path.into()],
            cwd: None,
            wait: false,
        }
    }
}

/// Launch candidates for `choice`. Empty for `System`, which the shell
/// handles with the OS opener.
pub fn editor_launches(os: &str, choice: &EditorChoice, path: &str) -> Vec<Launch> {
    let mac = os == "macos";
    match choice {
        EditorChoice::System => vec![],
        EditorChoice::VsCode => {
            let mut l = Vec::new();
            if mac {
                l.push(mac_app("Visual Studio Code", path));
            }
            l.push(cli(os, "code", path));
            l
        }
        EditorChoice::Cursor => {
            let mut l = Vec::new();
            if mac {
                l.push(mac_app("Cursor", path));
            }
            l.push(cli(os, "cursor", path));
            l
        }
        EditorChoice::Zed => {
            let mut l = Vec::new();
            if mac {
                l.push(mac_app("Zed", path));
            }
            l.push(cli(os, if os == "linux" { "zeditor" } else { "zed" }, path));
            if os == "linux" {
                l.push(cli(os, "zed", path));
            }
            l
        }
        EditorChoice::JetBrains => {
            let mut l = Vec::new();
            if mac {
                l.extend(JETBRAINS.iter().map(|(app, _)| mac_app(app, path)));
            }
            let mut seen = Vec::new();
            for (_, launcher) in JETBRAINS {
                if !seen.contains(launcher) {
                    seen.push(launcher);
                    l.push(cli(os, launcher, path));
                }
            }
            l
        }
        EditorChoice::Custom(template) => {
            if os == "windows" {
                let command = template.replace("{path}", &format!("\"{path}\""));
                vec![Launch {
                    program: "cmd.exe".into(),
                    args: vec!["/c".into(), command],
                    cwd: None,
                    wait: false,
                }]
            } else {
                let command = template.replace("{path}", &quote_sh(path));
                vec![Launch {
                    program: "sh".into(),
                    args: vec!["-c".into(), command],
                    cwd: None,
                    wait: false,
                }]
            }
        }
    }
}

/// Lexically normalise a path: resolve `.` and `..` without touching the
/// file system, so files that no longer exist can still be validated.
pub fn normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The path to open if `requested` lies inside `project_root`; `None`
/// otherwise. `None` for `requested` means the project itself.
pub fn validate_target(project_root: &Path, requested: Option<&str>) -> Option<PathBuf> {
    let root = normalise(project_root);
    let Some(requested) = requested else {
        return Some(root);
    };
    let candidate = Path::new(requested);
    let joined = if candidate.is_absolute() || requested.starts_with('/') {
        candidate.to_path_buf()
    } else {
        root.join(candidate)
    };
    let target = normalise(&joined);
    target.starts_with(&root).then_some(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_prefers_apps_then_cli() {
        let l = editor_launches("macos", &EditorChoice::VsCode, "/p/a.ts");
        assert_eq!(l[0].program, "open");
        assert_eq!(l[0].args, vec!["-a", "Visual Studio Code", "/p/a.ts"]);
        assert!(l[0].wait);
        assert_eq!(l[1].program, "code");
    }

    #[test]
    fn jetbrains_tries_phpstorm_first_and_dedupes_launchers() {
        let l = editor_launches("macos", &EditorChoice::JetBrains, "/p");
        assert_eq!(l[0].args[1], "PhpStorm");
        let idea = l.iter().filter(|x| x.program == "idea").count();
        assert_eq!(idea, 1);
        let linux = editor_launches("linux", &EditorChoice::JetBrains, "/p");
        assert!(linux.iter().all(|x| x.program != "open"));
    }

    #[test]
    fn windows_runs_launchers_through_cmd() {
        let l = editor_launches("windows", &EditorChoice::Cursor, r"C:\p\a.ts");
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].program, "cmd.exe");
        assert_eq!(l[0].args, vec!["/c", "cursor", r"C:\p\a.ts"]);
    }

    #[test]
    fn zed_on_linux_tries_zeditor_then_zed() {
        let l = editor_launches("linux", &EditorChoice::Zed, "/p");
        let programs: Vec<_> = l.iter().map(|x| x.program.as_str()).collect();
        assert_eq!(programs, vec!["zeditor", "zed"]);
    }

    #[test]
    fn custom_command_quotes_the_path() {
        let l = editor_launches(
            "macos",
            &EditorChoice::Custom("nvim-qt {path}".into()),
            "/it's/a b.ts",
        );
        assert_eq!(l[0].args[1], "nvim-qt '/it'\\''s/a b.ts'");
        assert!(editor_launches("linux", &EditorChoice::System, "/p").is_empty());
    }

    #[test]
    fn validates_paths_inside_the_project() {
        let root = Path::new("/Users/dev/web-shop");
        assert_eq!(
            validate_target(root, None),
            Some(PathBuf::from("/Users/dev/web-shop"))
        );
        assert_eq!(
            validate_target(root, Some("/Users/dev/web-shop/src/a.ts")),
            Some(PathBuf::from("/Users/dev/web-shop/src/a.ts"))
        );
        assert_eq!(
            validate_target(root, Some("src/./b.ts")),
            Some(PathBuf::from("/Users/dev/web-shop/src/b.ts"))
        );
        assert_eq!(
            validate_target(root, Some("/Users/dev/web-shop/../secrets.txt")),
            None
        );
        assert_eq!(validate_target(root, Some("/etc/passwd")), None);
        assert_eq!(
            validate_target(root, Some("/Users/dev/web-shop-other/x")),
            None,
            "prefix is not containment"
        );
    }

    #[test]
    fn editor_choice_serialises_with_kind_tag() {
        let json = serde_json::to_value(EditorChoice::JetBrains).unwrap();
        assert_eq!(json["kind"], "jet-brains");
        let custom: EditorChoice =
            serde_json::from_str(r#"{"kind":"custom","command":"x {path}"}"#).unwrap();
        assert_eq!(custom, EditorChoice::Custom("x {path}".into()));
    }
}
