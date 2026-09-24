//! GitHub integration: account, repositories, Actions workflows and runs.
//! The token is stored in the OS keychain and never handed to the frontend.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{proc, secrets};

pub const TOKEN_KEY: &str = "github-token";
const API: &str = "https://api.github.com";

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("easyDeploy")
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())
}

fn token() -> Result<String, String> {
    secrets::get(TOKEN_KEY).ok_or_else(|| "Nicht mit GitHub verbunden".to_string())
}

/// owner/repo/workflow are interpolated into API paths; restrict them to the
/// characters GitHub allows so nothing can escape the intended endpoint.
fn check_segment(kind: &str, v: &str) -> Result<(), String> {
    if v.is_empty() || v.starts_with('.') || !v.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) {
        return Err(format!("Ungültiger GitHub-{kind}: „{v}“"));
    }
    Ok(())
}

async fn api(method: reqwest::Method, path: &str, body: Option<Value>, tok: Option<&str>) -> Result<Value, String> {
    let tok = match tok {
        Some(t) => t.to_string(),
        None => token()?,
    };
    let mut req = client()?
        .request(method, format!("{API}{path}"))
        .bearer_auth(tok)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(b) = body {
        req = req.json(&b);
    }
    let resp = req.send().await.map_err(|e| format!("GitHub nicht erreichbar: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        let msg = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(String::from))
            .unwrap_or(text);
        return Err(format!("GitHub-Fehler {}: {}", status.as_u16(), msg));
    }
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GhUser {
    login: String,
    name: Option<String>,
    avatar_url: String,
}

fn user_from(v: &Value) -> GhUser {
    GhUser {
        login: v["login"].as_str().unwrap_or_default().into(),
        name: v["name"].as_str().map(String::from),
        avatar_url: v["avatar_url"].as_str().unwrap_or_default().into(),
    }
}

async fn validate_and_store(tok: &str) -> Result<GhUser, String> {
    let v = api(reqwest::Method::GET, "/user", None, Some(tok)).await?;
    secrets::set(TOKEN_KEY, tok)?;
    Ok(user_from(&v))
}

#[tauri::command]
pub async fn gh_connect_token(token: String) -> Result<GhUser, String> {
    validate_and_store(token.trim()).await
}

/// Imports the token from an installed & logged-in GitHub CLI (`gh auth token`).
#[tauri::command]
pub async fn gh_import_cli() -> Result<GhUser, String> {
    let out = tauri::async_runtime::spawn_blocking(|| proc::run("gh", &["auth", "token"], Duration::from_secs(10)))
        .await
        .map_err(|e| e.to_string())?
        .ok_or("GitHub CLI (gh) ist nicht installiert")?;
    if !out.ok {
        return Err("GitHub CLI ist nicht angemeldet – führe `gh auth login` aus".into());
    }
    validate_and_store(out.text.trim()).await
}

#[tauri::command]
pub async fn gh_status() -> Result<Option<GhUser>, String> {
    if secrets::get(TOKEN_KEY).is_none() {
        return Ok(None);
    }
    match api(reqwest::Method::GET, "/user", None, None).await {
        Ok(v) => Ok(Some(user_from(&v))),
        Err(e) if e.contains("401") => Ok(None),
        Err(e) => Err(e),
    }
}

#[tauri::command]
pub fn gh_logout() -> Result<(), String> {
    secrets::delete(TOKEN_KEY)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GhRepo {
    full_name: String,
    owner: String,
    name: String,
    private: bool,
    description: Option<String>,
    clone_url: String,
    ssh_url: String,
    html_url: String,
    default_branch: String,
    language: Option<String>,
    updated_at: String,
}

#[tauri::command]
pub async fn gh_repos() -> Result<Vec<GhRepo>, String> {
    let mut all = vec![];
    for page in 1..=3 {
        let v = api(
            reqwest::Method::GET,
            &format!("/user/repos?per_page=100&sort=updated&page={page}"),
            None,
            None,
        )
        .await?;
        let arr = v.as_array().cloned().unwrap_or_default();
        let n = arr.len();
        for r in arr {
            all.push(GhRepo {
                full_name: r["full_name"].as_str().unwrap_or_default().into(),
                owner: r["owner"]["login"].as_str().unwrap_or_default().into(),
                name: r["name"].as_str().unwrap_or_default().into(),
                private: r["private"].as_bool().unwrap_or(false),
                description: r["description"].as_str().map(String::from),
                clone_url: r["clone_url"].as_str().unwrap_or_default().into(),
                ssh_url: r["ssh_url"].as_str().unwrap_or_default().into(),
                html_url: r["html_url"].as_str().unwrap_or_default().into(),
                default_branch: r["default_branch"].as_str().unwrap_or("main").into(),
                language: r["language"].as_str().map(String::from),
                updated_at: r["updated_at"].as_str().unwrap_or_default().into(),
            });
        }
        if n < 100 {
            break;
        }
    }
    Ok(all)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GhWorkflow {
    id: u64,
    name: String,
    path: String,
    state: String,
    file: String,
}

#[tauri::command]
pub async fn gh_workflows(owner: String, repo: String) -> Result<Vec<GhWorkflow>, String> {
    check_segment("Owner", &owner)?;
    check_segment("Repository", &repo)?;
    let v = api(reqwest::Method::GET, &format!("/repos/{owner}/{repo}/actions/workflows?per_page=100"), None, None).await?;
    Ok(v["workflows"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|w| {
            let path = w["path"].as_str().unwrap_or_default().to_string();
            GhWorkflow {
                id: w["id"].as_u64().unwrap_or(0),
                name: w["name"].as_str().unwrap_or_default().into(),
                file: path.rsplit('/').next().unwrap_or_default().into(),
                path,
                state: w["state"].as_str().unwrap_or_default().into(),
            }
        })
        .collect())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GhRun {
    id: u64,
    name: String,
    title: String,
    status: String,
    conclusion: Option<String>,
    branch: String,
    event: String,
    html_url: String,
    created_at: String,
}

#[tauri::command]
pub async fn gh_runs(owner: String, repo: String) -> Result<Vec<GhRun>, String> {
    check_segment("Owner", &owner)?;
    check_segment("Repository", &repo)?;
    let v = api(reqwest::Method::GET, &format!("/repos/{owner}/{repo}/actions/runs?per_page=15"), None, None).await?;
    Ok(v["workflow_runs"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|r| GhRun {
            id: r["id"].as_u64().unwrap_or(0),
            name: r["name"].as_str().unwrap_or_default().into(),
            title: r["display_title"].as_str().unwrap_or_default().into(),
            status: r["status"].as_str().unwrap_or_default().into(),
            conclusion: r["conclusion"].as_str().map(String::from),
            branch: r["head_branch"].as_str().unwrap_or_default().into(),
            event: r["event"].as_str().unwrap_or_default().into(),
            html_url: r["html_url"].as_str().unwrap_or_default().into(),
            created_at: r["created_at"].as_str().unwrap_or_default().into(),
        })
        .collect())
}

pub async fn dispatch(
    owner: &str,
    repo: &str,
    workflow: &str,
    git_ref: &str,
    inputs: &HashMap<String, String>,
) -> Result<(), String> {
    check_segment("Owner", owner)?;
    check_segment("Repository", repo)?;
    check_segment("Workflow", workflow)?;
    let body = if inputs.is_empty() {
        json!({ "ref": git_ref })
    } else {
        json!({ "ref": git_ref, "inputs": inputs })
    };
    api(
        reqwest::Method::POST,
        &format!("/repos/{owner}/{repo}/actions/workflows/{workflow}/dispatches"),
        Some(body),
        None,
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn gh_dispatch(
    owner: String,
    repo: String,
    workflow: String,
    git_ref: String,
    inputs: Option<HashMap<String, String>>,
) -> Result<(), String> {
    dispatch(&owner, &repo, &workflow, &git_ref, &inputs.unwrap_or_default()).await
}
