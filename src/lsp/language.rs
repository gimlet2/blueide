//! Language detection and LSP server resolution.
//!
//! Given a file path this module determines:
//!   1. The LSP `languageId` string (used in `textDocument/didOpen`).
//!   2. Which LSP server binary to launch.
//!   3. Whether to fall back to a Docker-based server if the native binary is
//!      absent from the system PATH.

use std::path::Path;

// ---------------------------------------------------------------------------
// Language record
// ---------------------------------------------------------------------------

/// All information BlueIDE needs for one programming language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Language {
    /// LSP `languageId` (used in `textDocument/didOpen`).
    pub id: &'static str,
    /// Human-readable display name.
    pub name: &'static str,
    /// Native LSP server executable name (searched in PATH).
    pub native_cmd: &'static str,
    /// Extra CLI arguments for the native server.
    pub native_args: &'static [&'static str],
    /// Docker image to pull/run when the native binary is missing.
    pub docker_image: &'static str,
    /// Command inside the Docker container that starts the LSP server.
    pub docker_server_cmd: &'static [&'static str],
}

/// A mapping from one or more file extensions to a `Language`.
struct ExtMapping {
    exts: &'static [&'static str],
    language: Language,
}

// ---------------------------------------------------------------------------
// Language table
// ---------------------------------------------------------------------------

const LANGUAGES: &[ExtMapping] = &[
    ExtMapping {
        exts: &["kt", "kts"],
        language: Language {
            id: "kotlin",
            name: "Kotlin",
            native_cmd: "kotlin-language-server",
            native_args: &[],
            docker_image: "fwcd/kotlin-language-server:latest",
            docker_server_cmd: &["kotlin-language-server"],
        },
    },
    ExtMapping {
        exts: &["rs"],
        language: Language {
            id: "rust",
            name: "Rust",
            native_cmd: "rust-analyzer",
            native_args: &[],
            docker_image: "ghcr.io/rust-lang/rust-analyzer:latest",
            docker_server_cmd: &["rust-analyzer"],
        },
    },
    ExtMapping {
        exts: &["py", "pyw"],
        language: Language {
            id: "python",
            name: "Python",
            native_cmd: "pyright-langserver",
            native_args: &["--stdio"],
            docker_image: "blueide/pyright:latest",
            docker_server_cmd: &["pyright-langserver", "--stdio"],
        },
    },
    ExtMapping {
        exts: &["ts", "tsx"],
        language: Language {
            id: "typescript",
            name: "TypeScript",
            native_cmd: "typescript-language-server",
            native_args: &["--stdio"],
            docker_image: "blueide/typescript-ls:latest",
            docker_server_cmd: &["typescript-language-server", "--stdio"],
        },
    },
    ExtMapping {
        exts: &["js", "mjs", "cjs", "jsx"],
        language: Language {
            id: "javascript",
            name: "JavaScript",
            native_cmd: "typescript-language-server",
            native_args: &["--stdio"],
            docker_image: "blueide/typescript-ls:latest",
            docker_server_cmd: &["typescript-language-server", "--stdio"],
        },
    },
    ExtMapping {
        exts: &["go"],
        language: Language {
            id: "go",
            name: "Go",
            native_cmd: "gopls",
            native_args: &[],
            docker_image: "golang:latest",
            docker_server_cmd: &["gopls"],
        },
    },
    ExtMapping {
        exts: &["c", "h"],
        language: Language {
            id: "c",
            name: "C",
            native_cmd: "clangd",
            native_args: &[],
            docker_image: "blueide/clangd:latest",
            docker_server_cmd: &["clangd"],
        },
    },
    ExtMapping {
        exts: &["cpp", "cc", "cxx", "hpp", "hh", "hxx"],
        language: Language {
            id: "cpp",
            name: "C++",
            native_cmd: "clangd",
            native_args: &[],
            docker_image: "blueide/clangd:latest",
            docker_server_cmd: &["clangd"],
        },
    },
    ExtMapping {
        exts: &["java"],
        language: Language {
            id: "java",
            name: "Java",
            native_cmd: "jdtls",
            native_args: &[],
            docker_image: "blueide/jdtls:latest",
            docker_server_cmd: &["jdtls"],
        },
    },
    ExtMapping {
        exts: &["lua"],
        language: Language {
            id: "lua",
            name: "Lua",
            native_cmd: "lua-language-server",
            native_args: &[],
            docker_image: "blueide/lua-ls:latest",
            docker_server_cmd: &["lua-language-server"],
        },
    },
    ExtMapping {
        exts: &["sh", "bash"],
        language: Language {
            id: "shellscript",
            name: "Shell",
            native_cmd: "bash-language-server",
            native_args: &["start"],
            docker_image: "blueide/bash-ls:latest",
            docker_server_cmd: &["bash-language-server", "start"],
        },
    },
    ExtMapping {
        exts: &["yaml", "yml"],
        language: Language {
            id: "yaml",
            name: "YAML",
            native_cmd: "yaml-language-server",
            native_args: &["--stdio"],
            docker_image: "blueide/yaml-ls:latest",
            docker_server_cmd: &["yaml-language-server", "--stdio"],
        },
    },
    ExtMapping {
        exts: &["json", "jsonc"],
        language: Language {
            id: "json",
            name: "JSON",
            native_cmd: "vscode-json-language-server",
            native_args: &["--stdio"],
            docker_image: "blueide/vscode-ls:latest",
            docker_server_cmd: &["vscode-json-language-server", "--stdio"],
        },
    },
    ExtMapping {
        exts: &["html", "htm"],
        language: Language {
            id: "html",
            name: "HTML",
            native_cmd: "vscode-html-language-server",
            native_args: &["--stdio"],
            docker_image: "blueide/vscode-ls:latest",
            docker_server_cmd: &["vscode-html-language-server", "--stdio"],
        },
    },
    ExtMapping {
        exts: &["css", "scss", "sass", "less"],
        language: Language {
            id: "css",
            name: "CSS",
            native_cmd: "vscode-css-language-server",
            native_args: &["--stdio"],
            docker_image: "blueide/vscode-ls:latest",
            docker_server_cmd: &["vscode-css-language-server", "--stdio"],
        },
    },
    ExtMapping {
        exts: &["toml"],
        language: Language {
            id: "toml",
            name: "TOML",
            native_cmd: "taplo",
            native_args: &["lsp", "stdio"],
            docker_image: "blueide/taplo:latest",
            docker_server_cmd: &["taplo", "lsp", "stdio"],
        },
    },
    ExtMapping {
        exts: &["md", "markdown"],
        language: Language {
            id: "markdown",
            name: "Markdown",
            native_cmd: "marksman",
            native_args: &["server"],
            docker_image: "blueide/marksman:latest",
            docker_server_cmd: &["marksman", "server"],
        },
    },
];

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

/// Detect the language of a file from its path (extension-based).
///
/// Returns a reference to the matching [`Language`], or `None` for unknown
/// extensions.
pub fn detect_language(path: &Path) -> Option<&'static Language> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    LANGUAGES
        .iter()
        .find(|m| m.exts.iter().any(|e| *e == ext))
        .map(|m| &m.language)
}

// ---------------------------------------------------------------------------
// Server command resolution
// ---------------------------------------------------------------------------

/// How to launch an LSP server for a given file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerLaunch {
    /// Run the native binary directly.
    Native {
        /// Argv[0] — full path or bare executable name.
        cmd: String,
        /// Additional arguments.
        args: Vec<String>,
    },
    /// Run via `docker run --rm -i --network=none`.
    Docker {
        /// Docker image to use.
        image: String,
        /// Extra `--volume` mounts added automatically for the workspace.
        workspace_path: String,
        /// Command + args passed to the container.
        server_cmd: Vec<String>,
    },
}

impl ServerLaunch {
    /// Convert into an argv list that can be passed directly to
    /// [`tokio::process::Command`].
    ///
    /// For `Native` the first element is the executable, the rest are args.
    /// For `Docker` the full `docker run …` invocation is returned.
    pub fn into_argv(self) -> Vec<String> {
        match self {
            ServerLaunch::Native { cmd, args } => {
                let mut v = vec![cmd];
                v.extend(args);
                v
            }
            ServerLaunch::Docker { image, workspace_path, server_cmd } => {
                let mut v: Vec<String> = vec![
                    "docker".into(),
                    "run".into(),
                    "--rm".into(),
                    "-i".into(),
                    // Mount the workspace into the container at the same path.
                    "--volume".into(),
                    format!("{p}:{p}", p = workspace_path),
                    // Set the working directory inside the container.
                    "--workdir".into(),
                    workspace_path,
                    // No network access for the server (security).
                    "--network".into(),
                    "none".into(),
                    image,
                ];
                v.extend(server_cmd);
                v
            }
        }
    }
}

/// Resolve the best available server launch strategy for `lang`.
///
/// Checks whether `lang.native_cmd` exists in `PATH`.  If it does, returns
/// [`ServerLaunch::Native`].  If not *and* `docker` is available in `PATH`,
/// returns [`ServerLaunch::Docker`].  If neither is found, returns `None`.
pub fn resolve_server(
    lang: &Language,
    workspace_path: &str,
) -> Option<ServerLaunch> {
    if let Some(path) = which_in_path(lang.native_cmd) {
        return Some(ServerLaunch::Native {
            cmd: path.to_string_lossy().into_owned(),
            args: lang.native_args.iter().map(|s| s.to_string()).collect(),
        });
    }

    // Native binary absent — try Docker.
    if which_in_path("docker").is_some() {
        return Some(ServerLaunch::Docker {
            image: lang.docker_image.to_string(),
            workspace_path: workspace_path.to_string(),
            server_cmd: lang
                .docker_server_cmd
                .iter()
                .map(|s| s.to_string())
                .collect(),
        });
    }

    None
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

fn which_in_path(name: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let candidate = dir.join(name);
            if candidate.is_file() {
                Some(candidate)
            } else {
                None
            }
        })
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn kotlin_detected_from_kt_extension() {
        let lang = detect_language(Path::new("Main.kt")).unwrap();
        assert_eq!(lang.id, "kotlin");
        assert_eq!(lang.name, "Kotlin");
    }

    #[test]
    fn kotlin_detected_from_kts_extension() {
        let lang = detect_language(Path::new("build.kts")).unwrap();
        assert_eq!(lang.id, "kotlin");
    }

    #[test]
    fn rust_detected_from_rs_extension() {
        let lang = detect_language(Path::new("lib.rs")).unwrap();
        assert_eq!(lang.id, "rust");
        assert_eq!(lang.native_cmd, "rust-analyzer");
    }

    #[test]
    fn python_detected_from_py_extension() {
        let lang = detect_language(Path::new("script.py")).unwrap();
        assert_eq!(lang.id, "python");
    }

    #[test]
    fn typescript_detected_from_ts_extension() {
        let lang = detect_language(Path::new("app.ts")).unwrap();
        assert_eq!(lang.id, "typescript");
    }

    #[test]
    fn javascript_detected_from_js_extension() {
        let lang = detect_language(Path::new("index.js")).unwrap();
        assert_eq!(lang.id, "javascript");
    }

    #[test]
    fn go_detected_from_go_extension() {
        let lang = detect_language(Path::new("main.go")).unwrap();
        assert_eq!(lang.id, "go");
        assert_eq!(lang.native_cmd, "gopls");
    }

    #[test]
    fn c_detected_from_c_extension() {
        let lang = detect_language(Path::new("foo.c")).unwrap();
        assert_eq!(lang.id, "c");
        assert_eq!(lang.native_cmd, "clangd");
    }

    #[test]
    fn cpp_detected_from_cpp_extension() {
        let lang = detect_language(Path::new("foo.cpp")).unwrap();
        assert_eq!(lang.id, "cpp");
    }

    #[test]
    fn unknown_extension_returns_none() {
        assert!(detect_language(Path::new("file.xyz")).is_none());
    }

    #[test]
    fn no_extension_returns_none() {
        assert!(detect_language(Path::new("Makefile")).is_none());
    }

    #[test]
    fn extension_is_case_insensitive() {
        let lang = detect_language(Path::new("Main.KT")).unwrap();
        assert_eq!(lang.id, "kotlin");
    }

    #[test]
    fn native_launch_argv_has_no_docker_prefix() {
        let launch = ServerLaunch::Native {
            cmd: "/usr/bin/rust-analyzer".into(),
            args: vec![],
        };
        let argv = launch.into_argv();
        assert_eq!(argv[0], "/usr/bin/rust-analyzer");
        assert!(!argv.iter().any(|a| a == "docker"));
    }

    #[test]
    fn docker_launch_argv_starts_with_docker_run() {
        let lang = detect_language(Path::new("Main.kt")).unwrap();
        let launch = ServerLaunch::Docker {
            image: lang.docker_image.to_string(),
            workspace_path: "/workspace".to_string(),
            server_cmd: lang
                .docker_server_cmd
                .iter()
                .map(|s| s.to_string())
                .collect(),
        };
        let argv = launch.into_argv();
        assert_eq!(argv[0], "docker");
        assert_eq!(argv[1], "run");
        assert!(argv.contains(&lang.docker_image.to_string()));
        // The workspace must be mounted.
        assert!(argv.iter().any(|a| a.contains("/workspace")));
        // The server command must be the last elements.
        let last = argv.last().unwrap();
        assert_eq!(last, lang.docker_server_cmd.last().unwrap());
    }

    #[test]
    fn docker_argv_contains_network_none() {
        let launch = ServerLaunch::Docker {
            image: "some-image:latest".into(),
            workspace_path: "/proj".into(),
            server_cmd: vec!["lsp-server".into()],
        };
        let argv = launch.into_argv();
        let pos = argv.iter().position(|a| a == "--network").expect("--network flag");
        assert_eq!(argv[pos + 1], "none");
    }

    #[test]
    fn resolve_server_returns_none_when_neither_binary_nor_docker_found() {
        // Point PATH at an empty temp dir so nothing is found.
        let tmp = std::env::temp_dir();
        let saved = std::env::var_os("PATH").unwrap_or_default();
        std::env::set_var("PATH", &tmp);
        let lang = detect_language(Path::new("Main.kt")).unwrap();
        let result = resolve_server(lang, "/workspace");
        std::env::set_var("PATH", saved);
        assert!(result.is_none());
    }
}
