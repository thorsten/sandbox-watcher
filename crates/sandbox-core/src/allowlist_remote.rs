//! The allow list on a remote host: POSIX scripts that read and replace the
//! sandbox config through the SSH master, so `allowlist` can keep editing the
//! TOML locally. The write is a compare-and-swap on the file's SHA-256 under
//! `flock`, which replaces the lock span `allowlist::apply` holds locally.
//!
//! Every interpolated value is quoted with `ssh::shell_quote`; the new file
//! content travels base64-encoded.

use base64::Engine;

use crate::allowlist::ConfigTarget;
use crate::remote::REMOTE_CONFIG_PATH_SH;
use crate::runner::CommandOutput;
use crate::ssh::shell_quote;

/// SHA-256 of the empty string: what a missing file hashes to.
pub const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// What the preview script found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewReply {
    /// Project target without a `.sandbox` directory.
    Absent,
    /// A symlink where the rules forbid one.
    Symlink { path: String, reason: String },
    /// The file, or an empty content with [`EMPTY_SHA256`] when it is missing.
    Content {
        path: String,
        hash: String,
        content: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyReply {
    Ok,
    /// The file changed since the preview; nothing was written.
    Changed,
    /// Path rules refused the write.
    Refused(String),
}

/// Shell functions shared by both scripts. `resolve` sets `f` (file) and
/// `d` (directory) or prints `absent` / `symlink <path> <reason>` and exits.
fn prelude(target: ConfigTarget, project_root: Option<&str>) -> String {
    let locate = match target {
        ConfigTarget::Global => format!("f=\"{REMOTE_CONFIG_PATH_SH}\"; d=$(dirname \"$f\")"),
        ConfigTarget::Project => {
            let root = shell_quote(project_root.unwrap_or(""));
            format!("d={root}/.sandbox; f=\"$d\"/config.toml")
        }
    };
    let rules = match target {
        ConfigTarget::Project => {
            r#"
  [ -d "$d" ] || [ -L "$d" ] || { echo absent; exit 0; }
  [ -L "$d" ] && { echo "symlink $d is a symbolic link; refusing to write the project config"; exit 0; }
  [ -L "$f" ] && { echo "symlink $f is a symbolic link; refusing to write the project config"; exit 0; }
"#
        }
        ConfigTarget::Global => {
            r#"
  if [ -L "$f" ]; then
    real=$(readlink -f "$f" 2>/dev/null) || real=
    [ -n "$real" ] && [ -f "$real" ] || { echo "symlink $f does not point to a regular file"; exit 0; }
    f=$real; d=$(dirname "$f")
  elif [ -e "$f" ] && [ ! -f "$f" ]; then
    echo "symlink $f is not a regular file"; exit 0
  fi
"#
        }
    };
    format!(
        r#"set -u
hash() {{ if command -v sha256sum >/dev/null 2>&1; then sha256sum | cut -d' ' -f1; else shasum -a 256 | cut -d' ' -f1; fi; }}
{locate}
{rules}"#
    )
}

/// Print `path <f>` then either `missing` or `sha <hash>` and the content.
pub fn preview_script(target: ConfigTarget, project_root: Option<&str>) -> String {
    format!(
        r#"{}
echo "path $f"
if [ -e "$f" ]; then echo "sha $(hash < "$f")"; cat "$f"; else echo missing; fi
"#,
        prelude(target, project_root)
    )
}

/// Under a directory lock, re-hash the file, refuse with `changed` on a
/// mismatch, otherwise replace it atomically and print `ok`.
pub fn apply_script(
    target: ConfigTarget,
    project_root: Option<&str>,
    expected_hash: &str,
    new_content: &str,
) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(new_content);
    let expected = shell_quote(expected_hash);
    let encoded = shell_quote(&encoded);
    let mkdir = match target {
        ConfigTarget::Global => "mkdir -p \"$d\" || { echo \"refused cannot create $d\"; exit 0; }",
        ConfigTarget::Project => "",
    };
    format!(
        r#"{prelude}
{mkdir}
expected={expected}
write() {{
  if [ -e "$f" ]; then cur=$(hash < "$f"); else cur={empty}; fi
  [ "$cur" = "$expected" ] || {{ echo changed; return 0; }}
  tmp=$(mktemp "$d/.config.toml.XXXXXX") || {{ echo "refused cannot create a temporary file in $d"; return 0; }}
  if ! printf %s {encoded} | base64 -d > "$tmp"; then rm -f "$tmp"; echo "refused cannot write $tmp"; return 0; fi
  if [ -e "$f" ]; then
    chmod --reference="$f" "$tmp" 2>/dev/null || chmod "$(stat -c %a "$f" 2>/dev/null || stat -f %Lp "$f" 2>/dev/null)" "$tmp" 2>/dev/null || true
  fi
  if mv -f "$tmp" "$f"; then echo ok; else rm -f "$tmp"; echo "refused cannot replace $f"; fi
}}
if command -v flock >/dev/null 2>&1; then
  exec 9< "$d" && flock 9 && write
else
  lock="$d/.config.toml.lock"
  n=0
  until mkdir "$lock" 2>/dev/null; do
    n=$((n + 1))
    [ "$n" -lt 100 ] || {{ echo "refused cannot lock $d"; exit 0; }}
    sleep 0.1
  done
  write
  rmdir "$lock"
fi
"#,
        prelude = prelude(target, project_root),
        empty = EMPTY_SHA256,
    )
}

fn failed(output: &CommandOutput) -> Option<String> {
    (output.exit_code != 0).then(|| {
        let text = output.stderr.trim();
        if text.is_empty() {
            format!("remote script exited with {}", output.exit_code)
        } else {
            text.to_string()
        }
    })
}

pub fn parse_preview(output: &CommandOutput) -> std::result::Result<PreviewReply, String> {
    if let Some(e) = failed(output) {
        return Err(e);
    }
    let mut lines = output.stdout.splitn(3, '\n');
    let first = lines.next().unwrap_or("");
    if first == "absent" {
        return Ok(PreviewReply::Absent);
    }
    if let Some(rest) = first.strip_prefix("symlink ") {
        let (path, reason) = rest.split_once(' ').unwrap_or((rest, ""));
        return Ok(PreviewReply::Symlink {
            path: path.into(),
            reason: reason.into(),
        });
    }
    let path = first
        .strip_prefix("path ")
        .ok_or_else(|| format!("unexpected preview reply: {first}"))?
        .to_string();
    let second = lines.next().unwrap_or("");
    if second == "missing" {
        return Ok(PreviewReply::Content {
            path,
            hash: EMPTY_SHA256.into(),
            content: String::new(),
        });
    }
    let hash = second
        .strip_prefix("sha ")
        .ok_or_else(|| format!("unexpected preview reply: {second}"))?
        .to_string();
    Ok(PreviewReply::Content {
        path,
        hash,
        content: lines.next().unwrap_or("").to_string(),
    })
}

pub fn parse_apply(output: &CommandOutput) -> std::result::Result<ApplyReply, String> {
    if let Some(e) = failed(output) {
        return Err(e);
    }
    let line = output.stdout.lines().last().unwrap_or("").trim();
    match line {
        "ok" => Ok(ApplyReply::Ok),
        "changed" => Ok(ApplyReply::Changed),
        _ => match line
            .strip_prefix("refused ")
            .or_else(|| line.strip_prefix("symlink "))
        {
            Some(reason) => Ok(ApplyReply::Refused(reason.to_string())),
            None if line == "absent" => Ok(ApplyReply::Refused(
                "this project has no .sandbox directory".into(),
            )),
            None => Err(format!("unexpected apply reply: {line}")),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::CommandOutput;

    fn out(stdout: &str) -> CommandOutput {
        CommandOutput {
            exit_code: 0,
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    #[test]
    fn scripts_quote_every_value() {
        let root = "/home/dev/it's a proj\nline2";
        let s = preview_script(ConfigTarget::Project, Some(root));
        assert!(s.contains("'/home/dev/it'\\''s a proj\nline2'"), "{s}");
        assert!(
            !s.contains("/home/dev/it's a proj/"),
            "unquoted root leaked: {s}"
        );
        let a = apply_script(ConfigTarget::Project, Some(root), EMPTY_SHA256, "x = 1\n");
        assert!(a.contains("'/home/dev/it'\\''s a proj\nline2'"), "{a}");
        // Content is base64, so it never meets the shell.
        assert!(!a.contains("x = 1"), "{a}");
        assert!(a.contains("eCA9IDEK"), "{a}");
    }

    #[test]
    fn global_script_uses_sandbox_config_precedence() {
        let s = preview_script(ConfigTarget::Global, None);
        assert!(s.contains(crate::remote::REMOTE_CONFIG_PATH_SH), "{s}");
    }

    #[test]
    fn hash_command_falls_back_to_shasum() {
        let s = preview_script(ConfigTarget::Global, None);
        assert!(s.contains("sha256sum"), "{s}");
        assert!(s.contains("shasum -a 256"), "{s}");
    }

    #[test]
    fn parses_preview_replies() {
        assert!(matches!(
            parse_preview(&out("absent\n")).unwrap(),
            PreviewReply::Absent
        ));
        match parse_preview(&out("symlink /p/.sandbox is a symbolic link\n")).unwrap() {
            PreviewReply::Symlink { path, reason } => {
                assert_eq!(path, "/p/.sandbox");
                assert!(reason.contains("symbolic link"));
            }
            other => panic!("{other:?}"),
        }
        match parse_preview(&out("path /h/.config/sandbox/config.toml\nmissing\n")).unwrap() {
            PreviewReply::Content {
                path,
                hash,
                content,
            } => {
                assert_eq!(path, "/h/.config/sandbox/config.toml");
                assert_eq!(hash, EMPTY_SHA256);
                assert_eq!(content, "");
            }
            other => panic!("{other:?}"),
        }
        match parse_preview(&out("path /p/.sandbox/config.toml\nsha abc\n[x]\na = 1\n")).unwrap() {
            PreviewReply::Content { hash, content, .. } => {
                assert_eq!(hash, "abc");
                assert_eq!(content, "[x]\na = 1\n");
            }
            other => panic!("{other:?}"),
        }
        let failed = CommandOutput {
            exit_code: 1,
            stdout: String::new(),
            stderr: "sh: boom".into(),
        };
        assert_eq!(parse_preview(&failed).unwrap_err(), "sh: boom");
        assert!(parse_preview(&out("garbage\n")).is_err());
    }

    #[test]
    fn parses_apply_replies() {
        assert!(matches!(parse_apply(&out("ok\n")).unwrap(), ApplyReply::Ok));
        assert!(matches!(
            parse_apply(&out("changed\n")).unwrap(),
            ApplyReply::Changed
        ));
        match parse_apply(&out("refused /p/.sandbox is a symbolic link\n")).unwrap() {
            ApplyReply::Refused(r) => assert!(r.contains("symbolic link")),
            other => panic!("{other:?}"),
        }
        let failed = CommandOutput {
            exit_code: 1,
            stdout: String::new(),
            stderr: "mv: denied".into(),
        };
        assert_eq!(parse_apply(&failed).unwrap_err(), "mv: denied");
    }

    #[test]
    #[cfg(unix)]
    fn scripts_run_under_local_sh() {
        // The real scripts against a temp dir; skipped where sh is missing.
        if std::process::Command::new("sh")
            .arg("-c")
            .arg("true")
            .status()
            .is_err()
        {
            return;
        }
        let dir = tempdir();
        std::fs::create_dir(dir.join(".sandbox")).unwrap();
        let root = dir.to_str().unwrap();
        let run = |script: &str| {
            let o = std::process::Command::new("sh")
                .arg("-c")
                .arg(script)
                .output()
                .unwrap();
            CommandOutput {
                exit_code: o.status.code().unwrap_or(-1) as i64,
                stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
            }
        };
        let p = parse_preview(&run(&preview_script(ConfigTarget::Project, Some(root)))).unwrap();
        let PreviewReply::Content {
            hash,
            content,
            path,
        } = p
        else {
            panic!("{p:?}")
        };
        assert_eq!(content, "");
        assert_eq!(hash, EMPTY_SHA256);
        assert!(path.ends_with("/.sandbox/config.toml"));

        let a = run(&apply_script(
            ConfigTarget::Project,
            Some(root),
            &hash,
            "allow_network = [\"a\"]\n",
        ));
        assert!(matches!(parse_apply(&a).unwrap(), ApplyReply::Ok), "{a:?}");
        assert_eq!(
            std::fs::read_to_string(dir.join(".sandbox/config.toml")).unwrap(),
            "allow_network = [\"a\"]\n"
        );

        // Stale hash: refused, file untouched.
        let a = run(&apply_script(
            ConfigTarget::Project,
            Some(root),
            EMPTY_SHA256,
            "x = 1\n",
        ));
        assert!(
            matches!(parse_apply(&a).unwrap(), ApplyReply::Changed),
            "{a:?}"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join(".sandbox/config.toml")).unwrap(),
            "allow_network = [\"a\"]\n"
        );

        // Symlinked .sandbox: preview says symlink, apply refuses.
        let other = tempdir();
        std::os::unix::fs::symlink(dir.join(".sandbox"), other.join(".sandbox")).unwrap();
        let p = parse_preview(&run(&preview_script(
            ConfigTarget::Project,
            Some(other.to_str().unwrap()),
        )))
        .unwrap();
        assert!(matches!(p, PreviewReply::Symlink { .. }), "{p:?}");
        let a = run(&apply_script(
            ConfigTarget::Project,
            Some(other.to_str().unwrap()),
            EMPTY_SHA256,
            "x = 1\n",
        ));
        assert!(
            matches!(parse_apply(&a).unwrap(), ApplyReply::Refused(_)),
            "{a:?}"
        );
    }

    fn tempdir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sw-allow-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
