//! Integration tests for language detection and LSP server resolution.

use blueide::lsp::language::{detect_language, resolve_server, ServerLaunch};
use std::path::Path;

// ---------------------------------------------------------------------------
// detect_language
// ---------------------------------------------------------------------------

#[test]
fn detects_all_supported_languages() {
    let cases: &[(&str, &str)] = &[
        ("Main.kt",       "kotlin"),
        ("build.kts",     "kotlin"),
        ("lib.rs",        "rust"),
        ("script.py",     "python"),
        ("script.pyw",    "python"),
        ("app.ts",        "typescript"),
        ("App.tsx",       "typescript"),
        ("index.js",      "javascript"),
        ("module.mjs",    "javascript"),
        ("component.jsx", "javascript"),
        ("main.go",       "go"),
        ("foo.c",         "c"),
        ("foo.h",         "c"),
        ("foo.cpp",       "cpp"),
        ("foo.cc",        "cpp"),
        ("foo.hpp",       "cpp"),
        ("Hello.java",    "java"),
        ("mod.lua",       "lua"),
        ("run.sh",        "shellscript"),
        ("config.yaml",   "yaml"),
        ("config.yml",    "yaml"),
        ("package.json",  "json"),
        ("index.html",    "html"),
        ("style.css",     "css"),
        ("style.scss",    "css"),
        ("Cargo.toml",    "toml"),
        ("README.md",     "markdown"),
    ];

    for (file, expected_id) in cases {
        let lang = detect_language(Path::new(file))
            .unwrap_or_else(|| panic!("no language detected for {file}"));
        assert_eq!(
            lang.id, *expected_id,
            "expected {expected_id} for {file}, got {}",
            lang.id
        );
    }
}

#[test]
fn unknown_extensions_return_none() {
    let unknown = ["file.xyz", "data.bin", "archive.tar", "image.png", "Makefile"];
    for f in unknown {
        assert!(
            detect_language(Path::new(f)).is_none(),
            "expected None for {f}"
        );
    }
}

#[test]
fn detection_is_case_insensitive() {
    assert_eq!(detect_language(Path::new("MAIN.KT")).unwrap().id, "kotlin");
    assert_eq!(detect_language(Path::new("App.RS")).unwrap().id, "rust");
    assert_eq!(detect_language(Path::new("Script.PY")).unwrap().id, "python");
}

// ---------------------------------------------------------------------------
// ServerLaunch::into_argv
// ---------------------------------------------------------------------------

#[test]
fn native_launch_argv_first_element_is_executable() {
    let launch = ServerLaunch::Native {
        cmd: "/usr/local/bin/rust-analyzer".into(),
        args: vec![],
    };
    let argv = launch.into_argv();
    assert_eq!(argv[0], "/usr/local/bin/rust-analyzer");
    assert_eq!(argv.len(), 1);
}

#[test]
fn native_launch_argv_includes_extra_args() {
    let launch = ServerLaunch::Native {
        cmd: "pyright-langserver".into(),
        args: vec!["--stdio".into()],
    };
    let argv = launch.into_argv();
    assert_eq!(argv, vec!["pyright-langserver", "--stdio"]);
}

#[test]
fn docker_launch_argv_has_correct_structure() {
    let launch = ServerLaunch::Docker {
        image: "fwcd/kotlin-language-server:latest".into(),
        workspace_path: "/workspace/myproject".into(),
        server_cmd: vec!["kotlin-language-server".into()],
    };
    let argv = launch.into_argv();

    // Must start with "docker run".
    assert_eq!(&argv[..2], &["docker", "run"]);
    // Must contain --rm and -i.
    assert!(argv.contains(&"--rm".to_string()));
    assert!(argv.contains(&"-i".to_string()));
    // Must mount workspace.
    let vol_pos = argv.iter().position(|a| a == "--volume").expect("--volume");
    assert!(argv[vol_pos + 1].contains("/workspace/myproject"));
    // Must set workdir.
    let wd_pos = argv.iter().position(|a| a == "--workdir").expect("--workdir");
    assert_eq!(argv[wd_pos + 1], "/workspace/myproject");
    // Must use --network=none.
    let net_pos = argv.iter().position(|a| a == "--network").expect("--network");
    assert_eq!(argv[net_pos + 1], "none");
    // Image must appear.
    assert!(argv.contains(&"fwcd/kotlin-language-server:latest".to_string()));
    // Server command must be last.
    assert_eq!(argv.last().unwrap(), "kotlin-language-server");
}

// ---------------------------------------------------------------------------
// resolve_server — with controlled PATH
// ---------------------------------------------------------------------------

/// Run a closure with PATH overridden to a specific value, restoring it after.
fn with_path<R>(path_value: &str, f: impl FnOnce() -> R) -> R {
    let saved = std::env::var_os("PATH").unwrap_or_default();
    std::env::set_var("PATH", path_value);
    let result = f();
    std::env::set_var("PATH", saved);
    result
}

#[test]
fn resolve_returns_none_when_path_is_empty() {
    let lang = detect_language(Path::new("main.go")).unwrap();
    let result = with_path("", || resolve_server(lang, "/workspace"));
    assert!(result.is_none(), "expected None with empty PATH");
}

#[cfg(unix)]
#[test]
fn resolve_prefers_native_over_docker() {
    // Create a fake executable in a temp dir.
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().expect("tempdir");
    let fake_bin = tmp.path().join("rust-analyzer");
    std::fs::write(&fake_bin, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&fake_bin, std::fs::Permissions::from_mode(0o755)).unwrap();

    let lang = detect_language(Path::new("lib.rs")).unwrap();
    let result = with_path(
        &tmp.path().to_string_lossy(),
        || resolve_server(lang, "/workspace"),
    );

    match result {
        Some(ServerLaunch::Native { cmd, .. }) => {
            assert!(cmd.contains("rust-analyzer"), "expected rust-analyzer, got {cmd}");
        }
        other => panic!("expected Native, got {other:?}"),
    }
}
