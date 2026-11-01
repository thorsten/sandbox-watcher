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

/// Shell code shared by both scripts. `say_def` defines `say`, which reports
/// an early outcome (`absent` or `symlink <reason>`) and exits. The rules set
/// `f` (file) and `d` (directory) or call `say`.
fn prelude(target: ConfigTarget, project_root: Option<&str>, say_def: &str) -> String {
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
[ -d "$d" ] || [ -L "$d" ] || say absent
[ -L "$d" ] && say "symlink $d is a symbolic link; refusing to write the project config"
[ -L "$f" ] && say "symlink $f is a symbolic link; refusing to write the project config"
"#
        }
        ConfigTarget::Global => {
            r#"
if [ -L "$f" ]; then
  real=$(readlink -f "$f" 2>/dev/null) || real=
  [ -n "$real" ] && [ -f "$real" ] || say "symlink $f does not point to a regular file"
  f=$real; d=$(dirname "$f")
elif [ -e "$f" ] && [ ! -f "$f" ]; then
  say "symlink $f is not a regular file"
fi
"#
        }
    };
    format!(
        r#"set -u
if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
  echo "neither sha256sum nor shasum is installed on this host" >&2
  exit 1
fi
sha256_of() {{ if command -v sha256sum >/dev/null 2>&1; then sha256sum | cut -d' ' -f1; else shasum -a 256 | cut -d' ' -f1; fi; }}
{say_def}
{locate}
{rules}"#
    )
}

/// The remote command line for a script: the login shell on the host only
/// ever sees `sh -c '<script>'`, so fish or csh never parse the script.
fn as_remote_command(script: &str) -> String {
    format!("sh -c {}", shell_quote(script))
}

fn preview_body(target: ConfigTarget, project_root: Option<&str>) -> String {
    // The first line is always `path <f>`, the second the outcome.
    let say = r#"say() { printf 'path %s\n%s\n' "$f" "$1"; exit 0; }"#;
    format!(
        r#"{}
printf 'path %s\n' "$f"
if [ -e "$f" ]; then
  h=$(sha256_of < "$f") || exit 1
  printf 'sha %s\n' "$h"
  cat "$f"
else
  echo missing
fi
"#,
        prelude(target, project_root, say)
    )
}

/// The remote command line that reads the config. It prints `path <f>`, then
/// one of `absent`, `symlink <reason>`, `missing` or `sha <hash>` followed by
/// the file content.
pub fn preview_script(target: ConfigTarget, project_root: Option<&str>) -> String {
    as_remote_command(&preview_body(target, project_root))
}

fn apply_body(
    target: ConfigTarget,
    project_root: Option<&str>,
    expected_hash: &str,
    new_content: &str,
) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(new_content);
    let expected = shell_quote(expected_hash);
    let encoded = shell_quote(&encoded);
    let say = r#"say() { printf '%s\n' "$1"; exit 0; }"#;
    let mkdir = match target {
        ConfigTarget::Global => "mkdir -p \"$d\" || say \"refused cannot create $d\"",
        ConfigTarget::Project => "",
    };
    format!(
        r#"{prelude}
{mkdir}
expected={expected}
write() {{
  if [ -e "$f" ]; then cur=$(sha256_of < "$f") || exit 1; else cur={empty}; fi
  [ "$cur" = "$expected" ] || {{ echo changed; return 0; }}
  tmp=$(mktemp "$d/.config.toml.XXXXXX") || {{ printf 'refused cannot create a temporary file in %s\n' "$d"; return 0; }}
  if ! printf %s {encoded} | base64 -d > "$tmp"; then rm -f "$tmp"; printf 'refused cannot write %s\n' "$tmp"; return 0; fi
  if [ -e "$f" ]; then
    chmod --reference="$f" "$tmp" 2>/dev/null || chmod "$(stat -c %a "$f" 2>/dev/null || stat -f %Lp "$f" 2>/dev/null)" "$tmp" 2>/dev/null || true
  fi
  if mv -f "$tmp" "$f"; then echo ok; else rm -f "$tmp"; printf 'refused cannot replace %s\n' "$f"; fi
}}
if command -v flock >/dev/null 2>&1; then
  exec 9< "$d" && flock 9 && write
else
  lock="$d/.config.toml.lock"
  n=0
  until mkdir "$lock" 2>/dev/null; do
    n=$((n + 1))
    [ "$n" -lt 100 ] || {{ printf 'refused cannot lock %s\n' "$d"; exit 0; }}
    sleep 0.1
  done
  write
  rmdir "$lock"
fi
"#,
        prelude = prelude(target, project_root, say),
        empty = EMPTY_SHA256,
    )
}

/// The remote command line that, under a lock, re-hashes the file, prints
/// `changed` on a mismatch, otherwise replaces it atomically and prints `ok`.
/// Early refusals print `absent`, `symlink <reason>` or `refused <reason>`.
pub fn apply_script(
    target: ConfigTarget,
    project_root: Option<&str>,
    expected_hash: &str,
    new_content: &str,
) -> String {
    as_remote_command(&apply_body(
        target,
        project_root,
        expected_hash,
        new_content,
    ))
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
    let path = first
        .strip_prefix("path ")
        .ok_or_else(|| format!("unexpected preview reply: {first}"))?
        .to_string();
    let second = lines.next().unwrap_or("");
    if second == "absent" {
        return Ok(PreviewReply::Absent);
    }
    if let Some(reason) = second.strip_prefix("symlink ") {
        return Ok(PreviewReply::Symlink {
            path,
            reason: reason.into(),
        });
    }
    if second == "missing" {
        return Ok(PreviewReply::Content {
            path,
            hash: EMPTY_SHA256.into(),
            content: String::new(),
        });
    }
    let hash = second
        .strip_prefix("sha ")
        .ok_or_else(|| format!("unexpected preview reply: {second}"))?;
    if hash.len() != 64 || !hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return Err(format!("unexpected hash in preview reply: {hash}"));
    }
    Ok(PreviewReply::Content {
        path,
        hash: hash.into(),
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

    const HASH_A: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

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
        let s = preview_body(ConfigTarget::Project, Some(root));
        assert!(s.contains("'/home/dev/it'\\''s a proj\nline2'"), "{s}");
        assert!(
            !s.contains("/home/dev/it's a proj/"),
            "unquoted root leaked: {s}"
        );
        let a = apply_body(ConfigTarget::Project, Some(root), EMPTY_SHA256, "x = 1\n");
        assert!(a.contains("'/home/dev/it'\\''s a proj\nline2'"), "{a}");
        // Content is base64, so it never meets the shell.
        assert!(!a.contains("x = 1"), "{a}");
        assert!(a.contains("eCA9IDEK"), "{a}");
    }

    #[test]
    fn commands_are_wrapped_in_sh_c() {
        for c in [
            preview_script(ConfigTarget::Global, None),
            apply_script(ConfigTarget::Global, None, EMPTY_SHA256, ""),
        ] {
            assert!(c.starts_with("sh -c '"), "{c}");
            assert!(c.ends_with('\''), "{c}");
        }
    }

    #[test]
    fn global_script_uses_sandbox_config_precedence() {
        let s = preview_body(ConfigTarget::Global, None);
        assert!(s.contains(crate::remote::REMOTE_CONFIG_PATH_SH), "{s}");
    }

    #[test]
    fn hash_command_falls_back_to_shasum() {
        let s = preview_body(ConfigTarget::Global, None);
        assert!(s.contains("sha256sum"), "{s}");
        assert!(s.contains("shasum -a 256"), "{s}");
        assert!(s.contains("exit 1"), "{s}");
    }

    #[test]
    fn parses_preview_replies() {
        assert!(matches!(
            parse_preview(&out("path /p/.sandbox/config.toml\nabsent\n")).unwrap(),
            PreviewReply::Absent
        ));
        let reply = "path /my proj/.sandbox/config.toml\nsymlink /my proj/.sandbox is a symbolic link; refusing to write the project config\n";
        match parse_preview(&out(reply)).unwrap() {
            PreviewReply::Symlink { path, reason } => {
                assert_eq!(path, "/my proj/.sandbox/config.toml");
                assert_eq!(
                    reason,
                    "/my proj/.sandbox is a symbolic link; refusing to write the project config"
                );
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
        let reply = format!("path /p/.sandbox/config.toml\nsha {HASH_A}\n[x]\na = 1\n");
        match parse_preview(&out(&reply)).unwrap() {
            PreviewReply::Content { hash, content, .. } => {
                assert_eq!(hash, HASH_A);
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
    fn preview_rejects_malformed_hashes() {
        for bad in ["", "abc", &HASH_A.to_uppercase(), &format!("{HASH_A}0")] {
            let err = parse_preview(&out(&format!("path /p\nsha {bad}\nx\n"))).unwrap_err();
            assert!(
                err.contains("unexpected hash in preview reply"),
                "{bad}: {err}"
            );
        }
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

    #[cfg(unix)]
    fn run(command: &str, env: &[(&str, &std::path::Path)]) -> CommandOutput {
        // The returned string is a full command line, as the login shell sees it.
        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c").arg(command);
        for (k, v) in env {
            cmd.env(k, v);
        }
        let o = cmd.output().unwrap();
        CommandOutput {
            exit_code: o.status.code().unwrap_or(-1) as i64,
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        }
    }

    #[cfg(unix)]
    fn have_sh() -> bool {
        std::process::Command::new("sh")
            .arg("-c")
            .arg("true")
            .status()
            .is_ok()
    }

    #[cfg(unix)]
    fn content_of(reply: PreviewReply) -> (String, String, String) {
        match reply {
            PreviewReply::Content {
                path,
                hash,
                content,
            } => (path, hash, content),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    #[cfg(unix)]
    fn scripts_run_under_local_sh() {
        // The real scripts against a temp dir; skipped where sh is missing.
        if !have_sh() {
            return;
        }
        let dir = tempdir();
        std::fs::create_dir(dir.join(".sandbox")).unwrap();
        let root = dir.to_str().unwrap();
        let p = parse_preview(&run(
            &preview_script(ConfigTarget::Project, Some(root)),
            &[],
        ))
        .unwrap();
        let (path, hash, content) = content_of(p);
        assert_eq!(content, "");
        assert_eq!(hash, EMPTY_SHA256);
        assert!(path.ends_with("/.sandbox/config.toml"));

        let a = run(
            &apply_script(
                ConfigTarget::Project,
                Some(root),
                &hash,
                "allow_network = [\"a\"]\n",
            ),
            &[],
        );
        assert!(matches!(parse_apply(&a).unwrap(), ApplyReply::Ok), "{a:?}");
        let file = dir.join(".sandbox/config.toml");
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "allow_network = [\"a\"]\n"
        );

        // Stale hash: refused, file untouched.
        let a = run(
            &apply_script(ConfigTarget::Project, Some(root), EMPTY_SHA256, "x = 1\n"),
            &[],
        );
        assert!(
            matches!(parse_apply(&a).unwrap(), ApplyReply::Changed),
            "{a:?}"
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "allow_network = [\"a\"]\n"
        );

        // Symlinked .sandbox: preview says symlink, apply refuses.
        let other = tempdir();
        std::os::unix::fs::symlink(dir.join(".sandbox"), other.join(".sandbox")).unwrap();
        let other_root = other.to_str().unwrap();
        let p = parse_preview(&run(
            &preview_script(ConfigTarget::Project, Some(other_root)),
            &[],
        ))
        .unwrap();
        assert!(matches!(p, PreviewReply::Symlink { .. }), "{p:?}");
        let a = run(
            &apply_script(
                ConfigTarget::Project,
                Some(other_root),
                EMPTY_SHA256,
                "x = 1\n",
            ),
            &[],
        );
        assert!(
            matches!(parse_apply(&a).unwrap(), ApplyReply::Refused(_)),
            "{a:?}"
        );

        // No .sandbox at all.
        let bare = tempdir();
        let p = parse_preview(&run(
            &preview_script(ConfigTarget::Project, Some(bare.to_str().unwrap())),
            &[],
        ))
        .unwrap();
        assert!(matches!(p, PreviewReply::Absent), "{p:?}");
    }

    #[test]
    #[cfg(unix)]
    fn existing_file_round_trips_with_its_real_hash() {
        if !have_sh() {
            return;
        }
        let dir = tempdir();
        std::fs::create_dir(dir.join(".sandbox")).unwrap();
        let file = dir.join(".sandbox/config.toml");
        std::fs::write(&file, "allow_network = [\"a\"]\n").unwrap();
        let root = dir.to_str().unwrap();
        let (_, hash, content) = content_of(
            parse_preview(&run(
                &preview_script(ConfigTarget::Project, Some(root)),
                &[],
            ))
            .unwrap(),
        );
        assert_eq!(content, "allow_network = [\"a\"]\n");
        assert_ne!(hash, EMPTY_SHA256);
        assert_eq!(hash.len(), 64);
        let new =
            crate::allowlist::inject_allowed_domains(&content, &["b.example".to_string()]).unwrap();
        let a = run(
            &apply_script(ConfigTarget::Project, Some(root), &hash, &new),
            &[],
        );
        assert!(matches!(parse_apply(&a).unwrap(), ApplyReply::Ok), "{a:?}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), new);
        assert!(new.contains("b.example"));
    }

    #[test]
    #[cfg(unix)]
    fn project_root_with_space_and_quote() {
        if !have_sh() {
            return;
        }
        let base = tempdir();
        let dir = base.join("it's a proj");
        std::fs::create_dir_all(dir.join(".sandbox")).unwrap();
        let root = dir.to_str().unwrap();
        let (path, hash, _) = content_of(
            parse_preview(&run(
                &preview_script(ConfigTarget::Project, Some(root)),
                &[],
            ))
            .unwrap(),
        );
        assert_eq!(path, format!("{root}/.sandbox/config.toml"));
        let a = run(
            &apply_script(ConfigTarget::Project, Some(root), &hash, "x = 1\n"),
            &[],
        );
        assert!(matches!(parse_apply(&a).unwrap(), ApplyReply::Ok), "{a:?}");
        assert_eq!(
            std::fs::read_to_string(dir.join(".sandbox/config.toml")).unwrap(),
            "x = 1\n"
        );

        // A symlink under a path with a space keeps the path intact.
        let other = base.join("my link proj");
        std::fs::create_dir_all(&other).unwrap();
        std::os::unix::fs::symlink(dir.join(".sandbox"), other.join(".sandbox")).unwrap();
        let p = parse_preview(&run(
            &preview_script(ConfigTarget::Project, Some(other.to_str().unwrap())),
            &[],
        ))
        .unwrap();
        match p {
            PreviewReply::Symlink { path, .. } => {
                assert_eq!(path, format!("{}/.sandbox/config.toml", other.display()))
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    #[cfg(unix)]
    fn global_target_creates_missing_config_in_sandbox_config_dir() {
        if !have_sh() {
            return;
        }
        let dir = tempdir().join("conf dir");
        let env = [("SANDBOX_CONFIG_DIR", dir.as_path())];
        let (path, hash, content) = content_of(
            parse_preview(&run(&preview_script(ConfigTarget::Global, None), &env)).unwrap(),
        );
        assert_eq!(path, format!("{}/config.toml", dir.display()));
        assert_eq!(hash, EMPTY_SHA256);
        assert_eq!(content, "");
        let a = run(
            &apply_script(ConfigTarget::Global, None, &hash, "x = 1\n"),
            &env,
        );
        assert!(matches!(parse_apply(&a).unwrap(), ApplyReply::Ok), "{a:?}");
        assert_eq!(
            std::fs::read_to_string(dir.join("config.toml")).unwrap(),
            "x = 1\n"
        );
    }

    #[test]
    #[cfg(unix)]
    fn global_symlinked_config_is_written_through() {
        if !have_sh() {
            return;
        }
        let dir = tempdir();
        let elsewhere = tempdir();
        let target = elsewhere.join("real.toml");
        std::fs::write(&target, "a = 1\n").unwrap();
        let link = dir.join("config.toml");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let env = [("SANDBOX_CONFIG_DIR", dir.as_path())];
        let (path, hash, content) = content_of(
            parse_preview(&run(&preview_script(ConfigTarget::Global, None), &env)).unwrap(),
        );
        assert_eq!(content, "a = 1\n");
        // The reported path is the resolved target, not the link.
        assert_eq!(
            std::fs::canonicalize(&path).unwrap(),
            std::fs::canonicalize(&target).unwrap()
        );
        let a = run(
            &apply_script(ConfigTarget::Global, None, &hash, "a = 2\n"),
            &env,
        );
        assert!(matches!(parse_apply(&a).unwrap(), ApplyReply::Ok), "{a:?}");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "a = 2\n");
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[cfg(unix)]
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
