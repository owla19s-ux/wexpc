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
        return Err(ServiceError::new(
            "DISPATCH_REJECTED",
            "GitHub 操作未经匹配授权",
        ));
    }
    if token.is_empty() {
        return Err(ServiceError::new(
            "CREDENTIAL_UNAVAILABLE",
            "GitHub 凭证不可用",
        ));
    }
    if !safe_segment(owner)
        || !safe_segment(repo)
        || path.len() > 4096
        || path.is_empty()
        || path.split('/').any(|part| !safe_segment(part))
    {
        return Err(ServiceError::new("INVALID_INPUT", "仓库或文件路径无效"));
    }
    super::intent_binding::verify_arguments(
        dispatch,
        &[owner.as_bytes(), repo.as_bytes(), path.as_bytes()],
    )?;
    let mut url = Url::parse("https://api.github.com/")
        .map_err(|_| ServiceError::new("NETWORK_ERROR", "GitHub API 地址错误"))?;
    url.path_segments_mut()
        .map_err(|_| ServiceError::new("INVALID_INPUT", "路径无效"))?
        .extend(
            ["repos", owner, repo, "contents"]
                .into_iter()
                .chain(path.split('/')),
        );
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(45))
        .redirect(Policy::none())
        .build()
        .map_err(|_| ServiceError::new("NETWORK_ERROR", "无法创建 GitHub 请求"))?;
    let response = client
        .get(url)
        .header("User-Agent", "WEXPC/0.1.1")
        .header("Accept", "application/vnd.github+json")
        .bearer_auth(token)
        .send()
        .map_err(|_| ServiceError::new("NETWORK_ERROR", "GitHub 读取失败，未自动重试"))?;
    if !response.status().is_success() {
        return Err(ServiceError::new(
            "GITHUB_HTTP_ERROR",
            "GitHub 未返回成功结果，未执行其他操作",
        ));
    }
    let mut bytes = Vec::new();
    response
        .take(512 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ServiceError::new("GITHUB_RESPONSE_ERROR", "GitHub 内容读取失败"))?;
    if bytes.len() > 512 * 1024 {
        return Err(ServiceError::new(
            "TOOL_OUTPUT_LIMIT",
            "GitHub 文件元数据超过上限",
        ));
    }
    decode_file_content(&bytes)
}

fn decode_file_content(bytes: &[u8]) -> Result<String> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|_| ServiceError::new("GITHUB_RESPONSE_ERROR", "GitHub 内容响应格式无效"))?;
    if value["type"].as_str() != Some("file") {
        return Err(ServiceError::new(
            "TOOL_UNSUPPORTED_CONTENT",
            "GitHub 目标不是普通文件",
        ));
    }
    let sha = value["sha"]
        .as_str()
        .filter(|s| s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| ServiceError::new("GITHUB_RESPONSE_ERROR", "GitHub 未返回有效文件版本"))?;
    if value["encoding"].as_str() != Some("base64") {
        return Err(ServiceError::new(
            "TOOL_UNSUPPORTED_CONTENT",
            "不支持 GitHub 文件编码",
        ));
    }
    let encoded = value["content"]
        .as_str()
        .ok_or_else(|| ServiceError::new("GITHUB_RESPONSE_ERROR", "GitHub 文件内容缺失"))?;
    if encoded.len() > 128 * 1024 {
        return Err(ServiceError::new(
            "TOOL_OUTPUT_LIMIT",
            "GitHub 文件内容超过上限",
        ));
    }
    let compact: String = encoded
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    let raw = STANDARD
        .decode(compact)
        .map_err(|_| ServiceError::new("GITHUB_RESPONSE_ERROR", "GitHub 文件内容无法解码"))?;
    if raw.len() > 64 * 1024 {
        return Err(ServiceError::new(
            "TOOL_OUTPUT_LIMIT",
            "GitHub 文件超过回执上限",
        ));
    }
    let text = String::from_utf8(raw)
        .map_err(|_| ServiceError::new("TOOL_UNSUPPORTED_CONTENT", "GitHub 文件不是 UTF-8 文本"))?;
    serde_json::to_string(&serde_json::json!({"sha":sha,"content":text}))
        .map_err(|_| ServiceError::new("GITHUB_RESPONSE_ERROR", "无法编码 GitHub 读取结果"))
}

/// Update an existing GitHub file only with a pinned blob SHA.
/// The trusted Host must bind repo/path/content to the approved intent digest,
/// persist InFlight, and supply the GitHub credential. Never call on model data.
// Keep authorization, target version, payload and credential explicit at the Host boundary.
#[allow(clippy::too_many_arguments)]
pub fn update_approved_github_file(
    dispatch: &Dispatch,
    authorized_resource_id: &str,
    owner: &str,
    repo: &str,
    path: &str,
    expected_sha: &str,
    content: &[u8],
    token: &str,
) -> Result<String> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    if dispatch.intent.action != Action::Write
        || dispatch.intent.capability_id != "github_file_write"
        || dispatch.intent.resource_id != authorized_resource_id
        || dispatch.intent.expected_version.as_deref() != Some(expected_sha)
    {
        return Err(ServiceError::new(
            "DISPATCH_REJECTED",
            "GitHub 写入未获匹配授权",
        ));
    }
    if token.is_empty()
        || expected_sha.len() != 40
        || !expected_sha.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(ServiceError::new("INVALID_INPUT", "凭证或预期版本不可用"));
    }
    if !safe_segment(owner)
        || !safe_segment(repo)
        || path.is_empty()
        || path.len() > 4096
        || path.split('/').any(|p| !safe_segment(p))
        || content.len() > 256 * 1024
    {
        return Err(ServiceError::new("INVALID_INPUT", "目标路径或写入大小无效"));
    }
    super::intent_binding::verify_arguments(
        dispatch,
        &[
            owner.as_bytes(),
            repo.as_bytes(),
            path.as_bytes(),
            expected_sha.as_bytes(),
            content,
        ],
    )?;
    let mut url = Url::parse("https://api.github.com/")
        .map_err(|_| ServiceError::new("NETWORK_ERROR", "GitHub API 地址错误"))?;
    url.path_segments_mut()
        .map_err(|_| ServiceError::new("INVALID_INPUT", "路径无效"))?
        .extend(
            ["repos", owner, repo, "contents"]
                .into_iter()
                .chain(path.split('/')),
        );
    let body = serde_json::json!({
        "message": "WEXPC approved file update",
        "content": STANDARD.encode(content),
        "sha": expected_sha,
    });
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(60))
        .redirect(Policy::none())
        .build()
        .map_err(|_| ServiceError::new("NETWORK_ERROR", "无法创建 GitHub 请求"))?;
    let response = client
        .put(url.clone())
        .header("User-Agent", "WEXPC/0.1.1")
        .header("Accept", "application/vnd.github+json")
        .bearer_auth(token)
        .json(&body)
        .send()
        .map_err(|_| ServiceError::new("OUTCOME_UNKNOWN", "GitHub 更新结果不确定，不得自动重试"))?;
    if response.status().is_server_error() {
        return Err(ServiceError::new(
            "OUTCOME_UNKNOWN",
            "GitHub 服务端错误，写入结果未确认",
        ));
    }
    if !response.status().is_success() {
        return Err(ServiceError::new(
            "GITHUB_WRITE_REJECTED",
            "GitHub 未确认更新成功",
        ));
    }
    let value: serde_json::Value = response
        .json()
        .map_err(|_| ServiceError::new("OUTCOME_UNKNOWN", "GitHub 更新响应无法核验"))?;
    let sha = value
        .pointer("/content/sha")
        .and_then(|v| v.as_str())
        .filter(|s| s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| ServiceError::new("OUTCOME_UNKNOWN", "GitHub 未返回有效文件版本"))?;
    verify_written_content(&client, url, token, &value, content)
}

/// Create-only GitHub mutation. No expected SHA means the API must reject
/// an already-existing path; it cannot silently overwrite an existing file.
pub fn create_approved_github_file(
    dispatch: &Dispatch,
    authorized_resource_id: &str,
    owner: &str,
    repo: &str,
    path: &str,
    content: &[u8],
    token: &str,
) -> Result<String> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    if dispatch.intent.action != Action::Write
        || dispatch.intent.capability_id != "github_file_create"
        || dispatch.intent.resource_id != authorized_resource_id
        || dispatch.intent.expected_version.is_some()
    {
        return Err(ServiceError::new(
            "DISPATCH_REJECTED",
            "GitHub 文件创建未经匹配授权",
        ));
    }
    if token.is_empty()
        || !safe_segment(owner)
        || !safe_segment(repo)
        || path.len() > 4096
        || path.is_empty()
        || path.split('/').any(|segment| !safe_segment(segment))
        || content.len() > 4096
    {
        return Err(ServiceError::new(
            "INVALID_INPUT",
            "GitHub 创建路径或内容无效",
        ));
    }
    super::intent_binding::verify_arguments(
        dispatch,
        &[owner.as_bytes(), repo.as_bytes(), path.as_bytes(), content],
    )?;
    let mut url = Url::parse("https://api.github.com/")
        .map_err(|_| ServiceError::new("NETWORK_ERROR", "GitHub API 地址错误"))?;
    url.path_segments_mut()
        .map_err(|_| ServiceError::new("INVALID_INPUT", "路径无效"))?
        .extend(
            ["repos", owner, repo, "contents"]
                .into_iter()
                .chain(path.split('/')),
        );
    let body = serde_json::json!({
        "message": "WEXPC approved new file",
        "content": STANDARD.encode(content),
    });
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(60))
        .redirect(Policy::none())
        .build()
        .map_err(|_| ServiceError::new("NETWORK_ERROR", "无法创建 GitHub 请求"))?;
    let response = client
        .put(url.clone())
        .header("User-Agent", "WEXPC/0.1.1")
        .header("Accept", "application/vnd.github+json")
        .bearer_auth(token)
        .json(&body)
        .send()
        .map_err(|_| {
            ServiceError::new("OUTCOME_UNKNOWN", "GitHub 文件创建结果不确定，禁止自动重试")
        })?;
    if response.status().is_server_error() {
        return Err(ServiceError::new(
            "OUTCOME_UNKNOWN",
            "GitHub 服务异常，文件创建结果未知",
        ));
    }
    if response.status() != reqwest::StatusCode::CREATED {
        return Err(ServiceError::new(
            "GITHUB_WRITE_REJECTED",
            "GitHub 未确认创建成功，目标可能已存在",
        ));
    }
    let result: serde_json::Value = response
        .json()
        .map_err(|_| ServiceError::new("OUTCOME_UNKNOWN", "GitHub 创建响应无法核验"))?;
    let sha = result
        .pointer("/content/sha")
        .and_then(|v| v.as_str())
        .filter(|s| s.len() == 40 && s.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| ServiceError::new("OUTCOME_UNKNOWN", "GitHub 未返回新文件的有效版本"))?;
    verify_written_content(&client, url, token, &result, content)
}


/**
 * A successful PUT confirms that GitHub accepted the mutation, not that its
 * target bytes were independently checked. Read the immutable commit returned
 * by that PUT: a moving default branch would make a later read ambiguous.
 * A failed readback is UNKNOWN, never a reason to replay the PUT.
 */
fn verify_written_content(
    client: &Client,
    mut url: Url,
    token: &str,
    write_response: &serde_json::Value,
    expected_content: &[u8],
) -> Result<String> {
    let blob_sha = write_response
        .pointer("/content/sha")
        .and_then(|v| v.as_str())
        .filter(|s| valid_git_sha(s))
        .ok_or_else(|| ServiceError::new("OUTCOME_UNKNOWN", "GitHub 写入响应缺少有效文件版本"))?;
    let commit_sha = write_response
        .pointer("/commit/sha")
        .and_then(|v| v.as_str())
        .filter(|s| valid_git_sha(s))
        .ok_or_else(|| ServiceError::new("OUTCOME_UNKNOWN", "GitHub 写入响应缺少可回读的提交版本"))?;
    url.query_pairs_mut().append_pair("ref", commit_sha);
    let response = client
        .get(url)
        .header("User-Agent", "WEXPC/0.1.1")
        .header("Accept", "application/vnd.github+json")
        .bearer_auth(token)
        .send()
        .map_err(|_| ServiceError::new("OUTCOME_UNKNOWN", "GitHub 已接受写入，但提交回读失败；禁止自动重试"))?;
    if !response.status().is_success() {
        return Err(ServiceError::new(
            "OUTCOME_UNKNOWN",
            "GitHub 已接受写入，但提交版本不可回读；禁止自动重试",
        ));
    }
    let mut bytes = Vec::new();
    response
        .take(512 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ServiceError::new("OUTCOME_UNKNOWN", "GitHub 写入后回读不完整；禁止自动重试"))?;
    if bytes.len() > 512 * 1024 {
        return Err(ServiceError::new(
            "OUTCOME_UNKNOWN",
            "GitHub 写入后回读超过核验上限；禁止自动重试",
        ));
    }
    verify_readback_body(&bytes, blob_sha, expected_content)?;
    Ok(blob_sha.to_owned())
}

fn valid_git_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn verify_readback_body(body: &[u8], expected_sha: &str, expected_content: &[u8]) -> Result<()> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let value: serde_json::Value = serde_json::from_slice(body)
        .map_err(|_| ServiceError::new("OUTCOME_UNKNOWN", "GitHub 回读响应无法解析；禁止自动重试"))?;
    if value["type"].as_str() != Some("file")
        || value["sha"].as_str() != Some(expected_sha)
        || value["encoding"].as_str() != Some("base64")
    {
        return Err(ServiceError::new(
            "OUTCOME_UNKNOWN",
            "GitHub 回读目标类型或文件版本不一致；禁止自动重试",
        ));
    }
    let encoded = value["content"]
        .as_str()
        .ok_or_else(|| ServiceError::new("OUTCOME_UNKNOWN", "GitHub 回读缺少内容；禁止自动重试"))?;
    let compact: Vec<u8> = encoded
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    let actual = STANDARD
        .decode(compact)
        .map_err(|_| ServiceError::new("OUTCOME_UNKNOWN", "GitHub 回读内容编码无效；禁止自动重试"))?;
    if actual != expected_content {
        return Err(ServiceError::new(
            "OUTCOME_UNKNOWN",
            "GitHub 回读内容与已批准写入字节不一致；禁止自动重试",
        ));
    }
    Ok(())
}

fn safe_segment(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= 255
        && part != "."
        && part != ".."
        && part
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_readback_checks_blob_sha_and_exact_bytes() {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let original = b"line one\\nline two";
        let body = serde_json::json!({
            "type": "file",
            "sha": "a".repeat(40),
            "encoding": "base64",
            "content": STANDARD.encode(original),
        });
        let raw = body.to_string();
        assert!(verify_readback_body(raw.as_bytes(), &"a".repeat(40), original).is_ok());
        assert_eq!(
            verify_readback_body(raw.as_bytes(), &"b".repeat(40), original)
                .unwrap_err()
                .code,
            "OUTCOME_UNKNOWN"
        );
        assert_eq!(
            verify_readback_body(raw.as_bytes(), &"a".repeat(40), b"different")
                .unwrap_err()
                .code,
            "OUTCOME_UNKNOWN"
        );
    }

    #[test]
    fn pinned_readback_rejects_invalid_or_missing_content() {
        for value in [
            serde_json::json!({"type":"dir","sha":"a".repeat(40),"encoding":"base64","content":"YQ=="}),
            serde_json::json!({"type":"file","sha":"a".repeat(40),"encoding":"base64"}),
            serde_json::json!({"type":"file","sha":"a".repeat(40),"encoding":"base64","content":"%%%"}),
        ] {
            assert_eq!(
                verify_readback_body(value.to_string().as_bytes(), &"a".repeat(40), b"a")
                    .unwrap_err()
                    .code,
                "OUTCOME_UNKNOWN"
            );
        }
    }

    #[test]
    fn readback_ref_is_pinned_to_the_put_commit() {
        let mut url = Url::parse("https://api.github.com/repos/a/b/contents/README.md").unwrap();
        url.query_pairs_mut().append_pair("ref", &"b".repeat(40));
        assert_eq!(url.query_pairs().find(|(key, _)| key == "ref").unwrap().1, "b".repeat(40));
    }

    #[test]
    fn content_decoder_supplies_sha_for_followup_approved_write() {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let encoded = STANDARD.encode("versioned content");
        let body = serde_json::json!({
            "type": "file",
            "sha": "a".repeat(40),
            "encoding": "base64",
            "content": encoded
        });
        let decoded = decode_file_content(body.to_string().as_bytes()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&decoded).unwrap();
        assert_eq!(parsed["sha"], "a".repeat(40));
        assert_eq!(parsed["content"], "versioned content");
    }

    #[test]
    fn content_decoder_rejects_nonfile_or_invalid_sha() {
        let tree = serde_json::json!({"type":"dir", "sha":"a".repeat(40)});
        assert_eq!(
            decode_file_content(tree.to_string().as_bytes())
                .unwrap_err()
                .code,
            "TOOL_UNSUPPORTED_CONTENT"
        );
        let bad = serde_json::json!({
            "type":"file", "sha":"broken", "encoding":"base64", "content":"YWJj"
        });
        assert_eq!(
            decode_file_content(bad.to_string().as_bytes())
                .unwrap_err()
                .code,
            "GITHUB_RESPONSE_ERROR"
        );
    }

    #[test]
    fn create_requires_distinct_capability_and_exact_arguments() {
        use crate::digest_arguments;
        use wex_core::operation::OperationIntent;
        let target = "repo_identity";
        let mut intent = OperationIntent {
            id: "create_1".into(),
            actor_id: "actor".into(),
            conversation_id: "conv".into(),
            context_id: None,
            connection_id: "github".into(),
            capability_id: "github_file_create".into(),
            resource_id: target.into(),
            action: Action::Write,
            expected_version: None,
            intent_digest: digest_arguments(
                "github_file_create",
                target,
                &[b"owner", b"repo", b"new.txt", b"hello"],
            ),
        };
        let dispatch = Dispatch {
            intent: intent.clone(),
            attempt_id: "try".into(),
        };
        assert_eq!(
            create_approved_github_file(
                &dispatch, target, "owner", "repo", "new.txt", b"changed", "token"
            )
            .unwrap_err()
            .code,
            "INTENT_MISMATCH"
        );
        intent.expected_version = Some("a".repeat(40));
        let stale = Dispatch {
            intent,
            attempt_id: "try".into(),
        };
        assert_eq!(
            create_approved_github_file(
                &stale, target, "owner", "repo", "new.txt", b"hello", "token"
            )
            .unwrap_err()
            .code,
            "DISPATCH_REJECTED"
        );
    }

    #[test]
    fn refuses_missing_authorization_without_network() {
        let intent = wex_core::operation::OperationIntent {
            id: "one".into(),
            actor_id: "actor".into(),
            conversation_id: "conv".into(),
            context_id: None,
            connection_id: "github".into(),
            capability_id: "github_file_read".into(),
            resource_id: "repo_one".into(),
            action: Action::Read,
            expected_version: None,
            intent_digest: "0".repeat(64),
        };
        let dispatch = Dispatch {
            intent,
            attempt_id: "attempt".into(),
        };
        assert_eq!(
            read_approved_github_file(
                &dispatch,
                "another_repo",
                "owner",
                "repo",
                "README.md",
                "token"
            )
            .unwrap_err()
            .code,
            "DISPATCH_REJECTED"
        );
        assert_eq!(
            read_approved_github_file(
                &dispatch,
                "repo_one",
                "owner",
                "repo",
                "../private",
                "token"
            )
            .unwrap_err()
            .code,
            "INVALID_INPUT"
        );
        let mut write_dispatch = dispatch;
        write_dispatch.intent.action = Action::Write;
        write_dispatch.intent.capability_id = "github_file_write".into();
        write_dispatch.intent.expected_version = Some("a".repeat(40));
        assert_eq!(
            update_approved_github_file(
                &write_dispatch,
                "repo_one",
                "owner",
                "repo",
                "README.md",
                &"b".repeat(40),
                b"data",
                "token"
            )
            .unwrap_err()
            .code,
            "DISPATCH_REJECTED"
        );
    }
}
