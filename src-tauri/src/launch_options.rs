//! Launch command parsing, WINEDLLOVERRIDES merging, and 3-mode composition.

use crate::config::{GlobalLaunchOptions, LaunchCommandSource, LaunchEnvVar};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedLaunchCommand {
    pub env_vars: Vec<LaunchEnvVar>,
    pub wrappers: Vec<String>,
    pub args: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct LaunchComposeInput<'a> {
    pub source: LaunchCommandSource,
    pub launcher_options: Option<&'a str>,
    pub auto_env_vars: &'a [LaunchEnvVar],
    pub global: &'a GlobalLaunchOptions,
    pub use_global: bool,
    pub custom_env_vars: &'a [LaunchEnvVar],
    pub custom_wrapper: Option<&'a str>,
    pub custom_launch_args: Option<&'a str>,
    pub custom_raw_command: Option<&'a str>,
    /// When set (e.g. "eldenring"), Emperor launches via `me3 launch --game <name> --profile emperor.me3`.
    pub me3_game: Option<&'a str>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComposedLaunchCommand {
    pub env_vars: Vec<LaunchEnvVar>,
    pub wrappers: Vec<String>,
    pub args: Vec<String>,
    pub effective_command: String,
}

/// Tokenize a shell-like launch options string while preserving quoting on non-env tokens.
fn split_shell_tokens(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            if c == '\\' && q == '"' {
                if let Some(&next) = chars.peek() {
                    if next == '"' || next == '\\' {
                        current.push(c);
                        current.push(chars.next().unwrap());
                        continue;
                    }
                }
            }
            if c == q {
                quote = None;
                current.push(c);
            } else {
                current.push(c);
            }
        } else {
            match c {
                '"' | '\'' => {
                    quote = Some(c);
                    current.push(c);
                }
                c if c.is_whitespace() => {
                    if !current.is_empty() {
                        tokens.push(std::mem::take(&mut current));
                    }
                }
                _ => current.push(c),
            }
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn is_valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn unquote_env_value(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
    {
        let inner = &trimmed[1..trimmed.len() - 1];
        if trimmed.starts_with('"') {
            return inner.replace("\\\"", "\"").replace("\\\\", "\\");
        }
        return inner.to_string();
    }
    trimmed.to_string()
}

fn parse_env_assignment(token: &str) -> Option<LaunchEnvVar> {
    let eq = token.find('=')?;
    let name = &token[..eq];
    if !is_valid_env_name(name) {
        return None;
    }
    let val = unquote_env_value(&token[eq + 1..]);
    Some(LaunchEnvVar {
        name: name.to_string(),
        value: val,
    })
}

/// Parse a raw launch command string (e.g. Steam LaunchOptions or user raw add-on)
/// into environment variables, wrappers (before `%command%`), and arguments (after `%command%`).
pub fn parse_raw_launch_command(raw: &str) -> ParsedLaunchCommand {
    let mut out = ParsedLaunchCommand::default();
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return out;
    }

    let tokens = split_shell_tokens(trimmed);
    let cmd_idx = tokens.iter().position(|t| t == "%command%");

    match cmd_idx {
        Some(idx) => {
            let mut in_env = true;
            for tok in &tokens[..idx] {
                if in_env {
                    if let Some(env) = parse_env_assignment(tok) {
                        out.env_vars.push(env);
                        continue;
                    }
                    in_env = false;
                }
                out.wrappers.push(tok.clone());
            }
            for tok in &tokens[idx + 1..] {
                if tok != "%command%" {
                    out.args.push(tok.clone());
                }
            }
        }
        None => {
            let mut in_env = true;
            for tok in &tokens {
                if in_env {
                    if let Some(env) = parse_env_assignment(tok) {
                        out.env_vars.push(env);
                        continue;
                    }
                    in_env = false;
                }
                if tok.starts_with('-') {
                    out.args.push(tok.clone());
                } else {
                    out.wrappers.push(tok.clone());
                }
            }
        }
    }

    out
}

/// Merge two `WINEDLLOVERRIDES` values (e.g. `dinput8=n,b` + `winmm=n,b;foo=b`)
/// without losing existing DLL entries. Later entries update the mode for the same DLL.
pub fn merge_winedll_overrides(existing: &str, incoming: &str) -> String {
    let mut entries: Vec<(String, String)> = Vec::new();

    for source in [existing, incoming] {
        for group in source.split(';') {
            let group = group.trim();
            if group.is_empty() {
                continue;
            }
            let (dll_list, mode) = match group.split_once('=') {
                Some((lhs, rhs)) => (lhs.trim(), rhs.trim()),
                None => (group, ""),
            };
            for dll in dll_list.split(',') {
                let dll_clean = dll.trim();
                if dll_clean.is_empty() {
                    continue;
                }
                if let Some(pos) = entries
                    .iter()
                    .position(|(d, _)| d.eq_ignore_ascii_case(dll_clean))
                {
                    entries[pos].1 = mode.to_string();
                } else {
                    entries.push((dll_clean.to_string(), mode.to_string()));
                }
            }
        }
    }

    if entries.is_empty() {
        return String::new();
    }

    // Group DLLs that share the same mode in order of first appearance (e.g. `dinput8,winmm=n,b;foo=b`).
    let mut mode_order: Vec<String> = Vec::new();
    for (_, mode) in &entries {
        if !mode_order.iter().any(|m| m == mode) {
            mode_order.push(mode.clone());
        }
    }

    mode_order
        .into_iter()
        .map(|mode| {
            let dlls: Vec<&str> = entries
                .iter()
                .filter(|(_, m)| m == &mode)
                .map(|(d, _)| d.as_str())
                .collect();
            if mode.is_empty() {
                dlls.join(",")
            } else {
                format!("{}={}", dlls.join(","), mode)
            }
        })
        .collect::<Vec<_>>()
        .join(";")
}

fn push_env_var(target: &mut Vec<LaunchEnvVar>, incoming: LaunchEnvVar) {
    let name = incoming.name.trim();
    if name.is_empty() {
        return;
    }
    if name.eq_ignore_ascii_case("WINEDLLOVERRIDES") {
        if let Some(pos) = target
            .iter()
            .position(|v| v.name.eq_ignore_ascii_case("WINEDLLOVERRIDES"))
        {
            target[pos].value = merge_winedll_overrides(&target[pos].value, &incoming.value);
            return;
        }
        target.push(LaunchEnvVar {
            name: "WINEDLLOVERRIDES".to_string(),
            value: merge_winedll_overrides("", &incoming.value),
        });
        return;
    }
    if let Some(pos) = target.iter().position(|v| v.name == name) {
        target[pos].value = incoming.value;
    } else {
        target.push(LaunchEnvVar {
            name: name.to_string(),
            value: incoming.value,
        });
    }
}

fn push_unique_token(target: &mut Vec<String>, token: String) {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return;
    }
    if !target.iter().any(|existing| existing == trimmed) {
        target.push(trimmed.to_string());
    }
}

pub fn format_env_var(v: &LaunchEnvVar) -> String {
    if v.value.contains(' ')
        || v.value.contains('\t')
        || v.value.contains(';')
        || v.value.contains('\'')
        || v.value.contains('"')
    {
        let escaped = v.value.replace('\\', "\\\\").replace('"', "\\\"");
        format!("{}=\"{}\"", v.name, escaped)
    } else {
        format!("{}={}", v.name, v.value)
    }
}

/// Compose the final launch command across all 3 modes (`Launcher`, `Emperor`, `Merged`).
pub fn compose_launch_command(input: &LaunchComposeInput<'_>) -> ComposedLaunchCommand {
    if input.source == LaunchCommandSource::Launcher {
        let raw = input
            .launcher_options
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let parsed = raw.map(parse_raw_launch_command).unwrap_or_default();
        let effective_command = raw.map(str::to_string).unwrap_or_else(|| "%command%".to_string());
        return ComposedLaunchCommand {
            env_vars: parsed.env_vars,
            wrappers: parsed.wrappers,
            args: parsed.args,
            effective_command,
        };
    }

    let mut env_vars: Vec<LaunchEnvVar> = Vec::new();
    let mut wrappers: Vec<String> = Vec::new();
    let mut args: Vec<String> = Vec::new();

    // 1. Launcher options (only in Merged mode)
    if input.source == LaunchCommandSource::Merged {
        if let Some(raw) = input.launcher_options {
            #[cfg(target_os = "linux")]
            let cleaned = crate::tools::steam_launch::strip_emperor_block(raw);
            #[cfg(not(target_os = "linux"))]
            let cleaned = raw.to_string();
            let parsed = parse_raw_launch_command(&cleaned);
            for v in parsed.env_vars {
                push_env_var(&mut env_vars, v);
            }
            for w in parsed.wrappers {
                push_unique_token(&mut wrappers, w);
            }
            for a in parsed.args {
                push_unique_token(&mut args, a);
            }
        }
    }

    // 2. Auto-detected required env vars (WINEDLLOVERRIDES, tool overrides)
    for v in input.auto_env_vars {
        push_env_var(&mut env_vars, v.clone());
    }

    // 3. Global add-on launch commands (when enabled globally and opted in per-game)
    if input.global.enabled && input.use_global {
        if let Some(raw) = input.global.raw_command.as_deref() {
            let parsed = parse_raw_launch_command(raw);
            for v in parsed.env_vars {
                push_env_var(&mut env_vars, v);
            }
            for w in parsed.wrappers {
                push_unique_token(&mut wrappers, w);
            }
            for a in parsed.args {
                push_unique_token(&mut args, a);
            }
        }
        for v in &input.global.env_vars {
            push_env_var(&mut env_vars, v.clone());
        }
        if let Some(w) = input.global.wrapper.as_deref() {
            for tok in split_shell_tokens(w) {
                push_unique_token(&mut wrappers, tok);
            }
        }
        if let Some(a) = input.global.launch_args.as_deref() {
            for tok in split_shell_tokens(a) {
                push_unique_token(&mut args, tok);
            }
        }
    }

    // 4. Per-game custom launch commands
    if let Some(raw) = input.custom_raw_command {
        let parsed = parse_raw_launch_command(raw);
        for v in parsed.env_vars {
            push_env_var(&mut env_vars, v);
        }
        for w in parsed.wrappers {
            push_unique_token(&mut wrappers, w);
        }
        for a in parsed.args {
            push_unique_token(&mut args, a);
        }
    }
    for v in input.custom_env_vars {
        push_env_var(&mut env_vars, v.clone());
    }
    if let Some(w) = input.custom_wrapper {
        for tok in split_shell_tokens(w) {
            push_unique_token(&mut wrappers, tok);
        }
    }
    if let Some(a) = input.custom_launch_args {
        for tok in split_shell_tokens(a) {
            push_unique_token(&mut args, tok);
        }
    }

    let mut parts: Vec<String> = env_vars.iter().map(format_env_var).collect();
    parts.extend(wrappers.iter().cloned());
    if let Some(me3_game) = input.me3_game {
        parts.push(format!("me3 launch --game {me3_game} --profile emperor.me3"));
    } else {
        parts.push("%command%".to_string());
    }
    parts.extend(args.iter().cloned());

    ComposedLaunchCommand {
        env_vars,
        wrappers,
        args,
        effective_command: parts.join(" "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_raw_command_with_quoted_env_wrappers_and_args() {
        let parsed = parse_raw_launch_command(
            r#"MANGOHUD=1 WINEDLLOVERRIDES="dinput8=n,b;winmm=n,b" gamemoderun %command% --launcher-skip -dx12"#,
        );
        assert_eq!(parsed.env_vars.len(), 2);
        assert_eq!(parsed.env_vars[0].name, "MANGOHUD");
        assert_eq!(parsed.env_vars[0].value, "1");
        assert_eq!(parsed.env_vars[1].name, "WINEDLLOVERRIDES");
        assert_eq!(parsed.env_vars[1].value, "dinput8=n,b;winmm=n,b");
        assert_eq!(parsed.wrappers, vec!["gamemoderun"]);
        assert_eq!(parsed.args, vec!["--launcher-skip", "-dx12"]);
    }

    #[test]
    fn parses_raw_command_without_percent_command() {
        let parsed = parse_raw_launch_command("PROTON_ENABLE_WAYLAND=1 gamemoderun -dx12");
        assert_eq!(parsed.env_vars.len(), 1);
        assert_eq!(parsed.env_vars[0].name, "PROTON_ENABLE_WAYLAND");
        assert_eq!(parsed.wrappers, vec!["gamemoderun"]);
        assert_eq!(parsed.args, vec!["-dx12"]);
    }

    #[test]
    fn merges_winedlloverrides_without_losing_auto_dlls() {
        assert_eq!(
            merge_winedll_overrides("dinput8=n,b", "winmm=n,b"),
            "dinput8,winmm=n,b"
        );
        assert_eq!(
            merge_winedll_overrides("dinput8,winmm=n,b", "version=n;dinput8=n,b"),
            "dinput8,winmm=n,b;version=n"
        );
    }

    #[test]
    fn composes_emperor_only_with_global_and_custom_addons() {
        let auto = vec![LaunchEnvVar {
            name: "WINEDLLOVERRIDES".into(),
            value: "dinput8=n,b".into(),
        }];
        let global = GlobalLaunchOptions {
            enabled: true,
            env_vars: vec![LaunchEnvVar {
                name: "MANGOHUD".into(),
                value: "1".into(),
            }],
            wrapper: Some("gamemoderun".into()),
            launch_args: None,
            raw_command: Some("PROTON_ENABLE_WAYLAND=1 %command%".into()),
        };
        let composed = compose_launch_command(&LaunchComposeInput {
            source: LaunchCommandSource::Emperor,
            launcher_options: Some("STEAM_ONLY_VAR=1 %command% --steam-arg"),
            auto_env_vars: &auto,
            global: &global,
            use_global: true,
            custom_env_vars: &[LaunchEnvVar {
                name: "WINEDLLOVERRIDES".into(),
                value: "winmm=n,b".into(),
            }],
            custom_wrapper: Some("gamemoderun"),
            custom_launch_args: Some("--launcher-skip"),
            custom_raw_command: Some("DXVK_HUD=fps %command% -dx12"),
            me3_game: None,
        });

        // Steam-only var/arg must NOT appear in Emperor-only mode.
        assert!(!composed.effective_command.contains("STEAM_ONLY_VAR"));
        assert!(!composed.effective_command.contains("--steam-arg"));
        // Auto + custom WINEDLLOVERRIDES merged.
        assert!(composed
            .effective_command
            .contains("WINEDLLOVERRIDES=dinput8,winmm=n,b"));
        assert!(composed.effective_command.contains("MANGOHUD=1"));
        assert!(composed
            .effective_command
            .contains("PROTON_ENABLE_WAYLAND=1"));
        assert!(composed.effective_command.contains("DXVK_HUD=fps"));
        // Deduplicated wrapper and single %command%.
        assert_eq!(composed.wrappers, vec!["gamemoderun"]);
        assert_eq!(composed.effective_command.matches("%command%").count(), 1);
        assert_eq!(composed.args, vec!["-dx12", "--launcher-skip"]);
    }

    #[test]
    fn composes_merged_steam_plus_emperor() {
        let auto = vec![LaunchEnvVar {
            name: "WINEDLLOVERRIDES".into(),
            value: "dinput8=n,b".into(),
        }];
        let global = GlobalLaunchOptions {
            enabled: true,
            env_vars: vec![],
            wrapper: Some("gamemoderun".into()),
            launch_args: None,
            raw_command: None,
        };
        let composed = compose_launch_command(&LaunchComposeInput {
            source: LaunchCommandSource::Merged,
            launcher_options: Some("PROTON_ENABLE_WAYLAND=1 mangohud %command% -novid"),
            auto_env_vars: &auto,
            global: &global,
            use_global: true,
            custom_env_vars: &[],
            custom_wrapper: None,
            custom_launch_args: Some("--launcher-skip"),
            custom_raw_command: None,
            me3_game: None,
        });

        assert_eq!(
            composed.effective_command,
            "PROTON_ENABLE_WAYLAND=1 WINEDLLOVERRIDES=dinput8=n,b mangohud gamemoderun %command% -novid --launcher-skip"
        );
        assert_eq!(composed.effective_command.matches("%command%").count(), 1);
    }

    #[test]
    fn per_game_opt_out_skips_global_addons() {
        let global = GlobalLaunchOptions {
            enabled: true,
            env_vars: vec![LaunchEnvVar {
                name: "MANGOHUD".into(),
                value: "1".into(),
            }],
            wrapper: Some("gamemoderun".into()),
            launch_args: None,
            raw_command: None,
        };
        let composed = compose_launch_command(&LaunchComposeInput {
            source: LaunchCommandSource::Emperor,
            launcher_options: None,
            auto_env_vars: &[],
            global: &global,
            use_global: false,
            custom_env_vars: &[],
            custom_wrapper: None,
            custom_launch_args: None,
            custom_raw_command: None,
            me3_game: None,
        });
        assert_eq!(composed.effective_command, "%command%");
    }

    #[test]
    fn launcher_only_returns_launcher_options_verbatim() {
        let auto = vec![LaunchEnvVar {
            name: "WINEDLLOVERRIDES".into(),
            value: "dinput8=n,b".into(),
        }];
        let global = GlobalLaunchOptions {
            enabled: true,
            env_vars: vec![LaunchEnvVar {
                name: "MANGOHUD".into(),
                value: "1".into(),
            }],
            wrapper: None,
            launch_args: None,
            raw_command: None,
        };
        let composed = compose_launch_command(&LaunchComposeInput {
            source: LaunchCommandSource::Launcher,
            launcher_options: Some("PROTON_LOG=1 %command%"),
            auto_env_vars: &auto,
            global: &global,
            use_global: true,
            custom_env_vars: &[],
            custom_wrapper: Some("gamemoderun"),
            custom_launch_args: None,
            custom_raw_command: None,
            me3_game: None,
        });
        assert_eq!(composed.effective_command, "PROTON_LOG=1 %command%");
    }
}
