// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! cavehomectl get — read cluster resources from the running apiserver.
//!
//! The analogue of `kubectl get nodes` / `kubectl get pods`. This talks to the
//! unified binary's in-process apiserver (default `127.0.0.1:6443`) over a tiny
//! std HTTP/1.1 client — no async runtime, no TLS yet (the apiserver serves
//! plain HTTP; TLS on `:6443` is a follow-up). It prints the resource names so
//! an operator can confirm the home is up:
//!
//! ```text
//! $ cavehomectl get nodes
//! NAME
//! cave-home
//! ```

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use clap::{Arg, ArgMatches, Command};

/// Where the apiserver listens by default (the K3s port).
const DEFAULT_SERVER: &str = "127.0.0.1:6443";

#[must_use]
pub fn cmd() -> Command {
    Command::new("get")
        .about("Show cluster resources (nodes, pods) from the apiserver")
        .arg(
            Arg::new("server")
                .long("server")
                .global(true)
                .help("apiserver address as host:port")
                .default_value(DEFAULT_SERVER),
        )
        .subcommand(Command::new("nodes").about("List the machines that make up your home"))
        .subcommand(Command::new("pods").about("List the running workloads"))
}

pub fn run(matches: &ArgMatches, verbose: bool) -> i32 {
    let server = matches.get_one::<String>("server").map_or(DEFAULT_SERVER, String::as_str);
    let (kind, path) = match matches.subcommand() {
        Some(("nodes", _)) => ("nodes", "/api/v1/nodes"),
        Some(("pods", _)) => ("pods", "/api/v1/pods"),
        _ => {
            eprintln!("Usage: cavehomectl get <nodes|pods>");
            return 2;
        }
    };
    match fetch(server, path) {
        Ok(response) => {
            let body = extract_body(&response);
            let names = extract_names(body);
            print!("{}", render(kind, &names, verbose));
            0
        }
        Err(e) => {
            eprintln!("cavehomectl: cannot reach your home at {server}: {e}");
            eprintln!("Is it running? Start it with `cave-home server`.");
            1
        }
    }
}

/// Build the raw HTTP/1.1 GET request for `path` against `host`.
#[must_use]
fn build_request(host: &str, path: &str) -> String {
    format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nAccept: application/json\r\nConnection: close\r\n\r\n")
}

/// Open a connection, send the GET, and return the full response text.
fn fetch(server: &str, path: &str) -> Result<String, String> {
    let mut stream = TcpStream::connect(server).map_err(|e| e.to_string())?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).map_err(|e| e.to_string())?;
    stream.set_write_timeout(Some(Duration::from_secs(5))).map_err(|e| e.to_string())?;
    stream.write_all(build_request(server, path).as_bytes()).map_err(|e| e.to_string())?;
    let mut buf = String::new();
    stream.read_to_string(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

/// The body of an HTTP response (everything after the blank line).
#[must_use]
fn extract_body(response: &str) -> &str {
    response.split_once("\r\n\r\n").map_or("", |(_head, body)| body)
}

/// Pull every `metadata.name` out of a K8s list body. Each item carries exactly
/// one `"name":"…"` (in its metadata); addresses/conditions use `type`/`address`
/// keys, so a `"name"` scan uniquely identifies the resources. This is a
/// deliberately small extractor — a full typed client is a follow-up.
#[must_use]
fn extract_names(body: &str) -> Vec<String> {
    const MARKER: &str = "\"name\":\"";
    let mut names = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find(MARKER) {
        let after = &rest[start + MARKER.len()..];
        if let Some(end) = after.find('"') {
            names.push(after[..end].to_string());
            rest = &after[end + 1..];
        } else {
            break;
        }
    }
    names
}

/// Render the resource names as a simple table (kubectl-style).
#[must_use]
fn render(kind: &str, names: &[String], verbose: bool) -> String {
    if names.is_empty() {
        return format!("No {kind} found.\n");
    }
    let mut out = String::from("NAME\n");
    for name in names {
        out.push_str(name);
        out.push('\n');
    }
    if verbose {
        out.push_str(&format!("({} {kind})\n", names.len()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_targets_the_path_and_host() {
        let req = build_request("127.0.0.1:6443", "/api/v1/nodes");
        assert!(req.starts_with("GET /api/v1/nodes HTTP/1.1\r\n"), "{req}");
        assert!(req.contains("Host: 127.0.0.1:6443\r\n"));
        assert!(req.contains("Connection: close\r\n"));
    }

    #[test]
    fn body_is_split_from_the_head() {
        let resp = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"items\":[]}";
        assert_eq!(extract_body(resp), "{\"items\":[]}");
    }

    #[test]
    fn extracts_node_name_from_a_nodelist() {
        let body = r#"{"apiVersion":"v1","items":[{"kind":"Node","metadata":{"name":"cave-home","uid":"uid-1"},"status":{"addresses":[{"address":"10.0.0.5","type":"InternalIP"}],"conditions":[{"type":"Ready"}]}}],"kind":"NodeList"}"#;
        assert_eq!(extract_names(body), vec!["cave-home".to_string()]);
    }

    #[test]
    fn empty_list_yields_no_names() {
        let body = r#"{"apiVersion":"v1","items":[],"kind":"PodList"}"#;
        assert!(extract_names(body).is_empty());
    }

    #[test]
    fn render_lists_names_with_header() {
        let out = render("nodes", &["cave-home".to_string()], false);
        assert!(out.starts_with("NAME\n"), "{out}");
        assert!(out.contains("cave-home"), "{out}");
    }

    #[test]
    fn render_empty_is_friendly() {
        assert_eq!(render("pods", &[], false), "No pods found.\n");
    }

    #[test]
    fn cmd_exposes_nodes_and_pods() {
        let names: Vec<_> = cmd().get_subcommands().map(|s| s.get_name().to_string()).collect();
        assert!(names.iter().any(|n| n == "nodes"));
        assert!(names.iter().any(|n| n == "pods"));
    }
}
