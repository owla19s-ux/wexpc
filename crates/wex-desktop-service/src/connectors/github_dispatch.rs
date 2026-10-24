//! GitHub content read adapter. Calls are never sent from raw model output.
use crate::{Result, ServiceError};
use reqwest::{blocking::Client, redirect::Policy, Url};
use std::{io::Read, time::Duration};
use wex_core::operation::{Action, Dispatch};

/// Repository identity and path come from the trusted Host approval snapshot.
/// The intent resource ID must exactly match that trusted repository record.
pub fn read_approved_github_file(
    dispatch: &Dispatch,
    authorized_resource_id: &str,
    owner: &str,
    repo: &str,
    path: &str,
    token: &str,
) -> Result<String> {
    if dispatch.intent.action != Action::Read
        || dispatch.intent.capability_id != "github_file_read"
        || dispatch.intent.resource_id != authorized_resource_id
    {
        return Err(ServiceError::new("DISPATCH_REJECTED", "GitHub 操作未经匹配授权"));
    }
    if token.is_empty() {
        return Err(ServiceError::new("CREDENTIAL_UNAVAILABLE", "GitHub 凭证不可用"));
    }
    if !safe_segment(owner) || !safe_segment(repo)
        || path.len() > 4096 || path.is_empty()
        || path.split('/').any(|part| !safe_segment(part))
    {
        return Err(ServiceError::new("INVALID_INPUT", "仓库或文件路径无效"));
    }
    let mut url = Url::parse("https://api.github.com/repos/")
        .map_err(|_| ServiceError::new("NETWORK_ERROR", "GitHub API 地址错误"))?;
    url.path_segments_mut().map_err(|_| ServiceError::new("INVALID_INPUT", "路径无效"))?
        .extend(["repos", owner, repo, "contents"].into_iter().chain(path.split('/')));
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(45))
        .redirect(Policy::none())
        .build()
        .map_err(|_| ServiceError::new("NETWORK_ERROR", "无法创建 GitHub 请求"))?;
    let response = client.get(url)
        .header("User-Agent", "WEXPC/0.1.1")
        .header("Accept", "application/vnd.github.raw+json")
        .bearer_auth(token)
        .send()
        .map_err(|_| ServiceError::new("NETWORK_ERROR", "GitHub 读取失败，未自动重试"))?;
    if !response.status().is_success() {
        return Err(ServiceError::new("GITHUB_HTTP_ERROR", "GitHub 未返回成功结果，未执行其他操作"));
    }
    let mut bytes = Vec::new();
    response.take(64 * 1024 + 1).read_to_end(&mut bytes)
        .map_err(|_| ServiceError::new("GITHUB_RESPONSE_ERROR", "GitHub 内容读取失败"))?;
    if bytes.len() > 64 * 1024 {
        return Err(ServiceError::new("TOOL_OUTPUT_LIMIT", "GitHub 文件超过回执上限"));
    }
    String::from_utf8(bytes)
        .map_err(|_| ServiceError::new("TOOL_UNSUPPORTED_CONTENT", "GitHub 文件不是 UTF-8 文本"))
}

fn safe_segment(part: &str) -> bool {
    !part.is_empty() && part.len() <= 255
        && part != "." && part != ".."
        && part.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_missing_authorization_without_network() {
        let intent = wex_core::operation::OperationIntent {
            id: "one".into(), actor_id: "actor".into(), conversation_id: "conv".into(),
            context_id: None, connection_id: "github".into(),
            capability_id: "github_file_read".into(), resource_id: "repo_one".into(),
            action: Action::Read, expected_version: None, intent_digest: "0".repeat(64),
        };
        let dispatch = Dispatch { intent, attempt_id: "attempt".into() };
        assert_eq!(
            read_approved_github_file(&dispatch, "another_repo", "owner", "repo", "README.md", "token")
                .unwrap_err().code, "DISPATCH_REJECTED"
        );
        assert_eq!(
            read_approved_github_file(&dispatch, "repo_one", "owner", "repo", "../private", "token")
                .unwrap_err().code, "INVALID_INPUT"
        );
    }
}
