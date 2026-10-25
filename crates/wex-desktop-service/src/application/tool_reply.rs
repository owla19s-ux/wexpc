use super::*;

fn candidate(
    s: &DesktopSnapshot,
    conversation: &str,
    turn: &str,
    recovering: bool,
) -> Result<PreparedChat> {
    let t = s
        .turns
        .iter()
        .find(|t| t.id == turn && t.conversation_id == conversation)
        .ok_or_else(|| ServiceError::new("TURN_NOT_FOUND", "原对话回合不存在"))?;
    let allowed = if recovering {
        matches!(t.status, TurnStatus::Failed | TurnStatus::Interrupted)
    } else {
        t.status == TurnStatus::Running
    };
    if !allowed {
        return Err(ServiceError::new(
            "INVALID_STATE",
            "该回合已完成或正在处理，未重复恢复",
        ));
    }
    if s.turns.iter().any(|other| {
        other.conversation_id == conversation
            && other.id != turn
            && other.status == TurnStatus::Running
    }) {
        return Err(ServiceError::new(
            "CONVERSATION_BUSY",
            "当前对话已有请求正在处理",
        ));
    }
    let selected = catalog(s)?
        .resolve_conversation(conversation)
        .map_err(|_| ServiceError::new("CONNECTION_UNAVAILABLE", "原对话连接或模型不可用"))?;
    selection(s, &selected.connection_id, &selected.model_id)?;
    let c = s
        .connections
        .iter()
        .find(|c| c.id == selected.connection_id)
        .ok_or_else(|| invalid("连接不存在"))?;
    Ok(PreparedChat {
        turn_id: turn.into(),
        conversation_id: conversation.into(),
        connection_id: c.id.clone(),
        credential_ref: c.credential_ref.clone(),
        model_id: selected.model_id,
        base_url: c.base_url.clone(),
        messages: vec![ApiMessage {
            role: "user".into(),
            content: t.user.clone(),
        }],
    })
}
fn matches_binding(current: &PreparedChat, frozen: &PreparedChat) -> bool {
    current.connection_id == frozen.connection_id
        && current.model_id == frozen.model_id
        && current.base_url == frozen.base_url
        && frozen
            .messages
            .last()
            .is_some_and(|m| m.role == "user" && m.content == current.messages[0].content)
}
impl DesktopService {
    /// Keep a disk copy before SQLite settlement. If both writes fail, the
    /// existing pending-results path still owns the complete in-memory reply.
    pub fn finish_chat_with_backup(
        &mut self,
        root: &Path,
        request: &PreparedChat,
        response: String,
    ) -> Result<()> {
        let state = self.store.load()?;
        let turn = state
            .turns
            .iter()
            .find(|t| t.id == request.turn_id && t.conversation_id == request.conversation_id)
            .ok_or_else(|| ServiceError::new("TURN_NOT_FOUND", "原回合不存在"))?;
        if turn.status != TurnStatus::Running
            || !request
                .messages
                .last()
                .is_some_and(|m| m.role == "user" && m.content == turn.user)
        {
            return Err(ServiceError::new(
                "INVALID_STATE",
                "原回合已变化，未保存其他请求的回复",
            ));
        }
        let backup = save_chat_reply(root, request, &response);
        let result = self.finish_chat(&request.turn_id, response);
        if matches!(&result,Err(error) if error.code=="STORAGE_ERROR") && backup.is_err() {
            return Err(ServiceError::new(
                "STORAGE_ERROR",
                "最终回复的本地备份与数据库保存均失败，结果仍留在内存；请保持应用打开并重试保存",
            ));
        }
        result
    }
    /// Local result recovery does not depend on provider enablement.
    pub fn saved_reply_candidate(&self, conversation: &str, turn: &str) -> Result<String> {
        if self.pending_results.contains_key(turn) {
            return Err(ServiceError::new(
                "UNSAVED_RESULT",
                "已有待保存结果，请先重试保存",
            ));
        }
        let s = self.store.load()?;
        let t = s
            .turns
            .iter()
            .find(|t| t.id == turn && t.conversation_id == conversation)
            .ok_or_else(|| ServiceError::new("TURN_NOT_FOUND", "原对话回合不存在"))?;
        if !matches!(t.status, TurnStatus::Failed | TurnStatus::Interrupted) {
            return Err(ServiceError::new(
                "INVALID_STATE",
                "该回合已完成或正在处理，未覆盖先前结果",
            ));
        }
        if s.turns
            .iter()
            .any(|t| t.conversation_id == conversation && t.status == TurnStatus::Running)
        {
            return Err(ServiceError::new(
                "CONVERSATION_BUSY",
                "当前对话有请求正在处理",
            ));
        }
        Ok(t.user.clone())
    }
    pub fn restore_saved_reply(
        &mut self,
        conversation: &str,
        turn: &str,
        original_user: &str,
        response: String,
    ) -> Result<()> {
        if self.saved_reply_candidate(conversation, turn)? != original_user {
            return Err(ServiceError::new(
                "INVALID_STATE",
                "原请求内容已变化，未恢复其他请求的回复",
            ));
        }
        self.mutate(|s| {
            let t = s
                .turns
                .iter_mut()
                .find(|t| t.id == turn && t.conversation_id == conversation)
                .ok_or_else(|| invalid("原回合不存在"))?;
            if !matches!(t.status, TurnStatus::Failed | TurnStatus::Interrupted)
                || t.user != original_user
            {
                return Err(ServiceError::new(
                    "INVALID_STATE",
                    "回合已变化，未覆盖先前结果",
                ));
            }
            t.assistant = Some(response);
            t.status = TurnStatus::Succeeded;
            t.error = None;
            Ok(())
        })
    }
    /// Read-only preflight. It neither creates a turn nor reads a credential.
    pub fn tool_reply_candidate(&self, conversation: &str, turn: &str) -> Result<PreparedChat> {
        if self.pending_results.contains_key(turn) {
            return Err(ServiceError::new(
                "UNSAVED_RESULT",
                "已有待保存结果，请先重试保存，避免重新请求模型",
            ));
        }
        candidate(&self.store.load()?, conversation, turn, true)
    }
    /// Admit the original failed/interrupted turn after restoring its checkpoint.
    pub fn begin_tool_reply_resume(&mut self, frozen: PreparedChat) -> Result<PreparedChat> {
        if self.pending_results.contains_key(&frozen.turn_id) {
            return Err(ServiceError::new(
                "UNSAVED_RESULT",
                "已有待保存结果，请先重试保存",
            ));
        }
        self.mutate(|s| {
            let current = candidate(s, &frozen.conversation_id, &frozen.turn_id, true)?;
            if !matches_binding(&current, &frozen) {
                return Err(ServiceError::new(
                    "CONNECTION_CHANGED",
                    "原请求绑定已变化，未向其他连接或模型发送回执",
                ));
            }
            let mut request = frozen;
            request.credential_ref = current.credential_ref;
            let turn = s
                .turns
                .iter_mut()
                .find(|t| t.id == request.turn_id)
                .ok_or_else(|| invalid("回合不存在"))?;
            turn.status = TurnStatus::Running;
            turn.error = None;
            Ok(request)
        })
    }
    /// Recheck immediately before network dispatch, after admission released the mutex.
    pub fn check_tool_reply_binding(&self, frozen: &PreparedChat) -> Result<()> {
        let current = candidate(
            &self.store.load()?,
            &frozen.conversation_id,
            &frozen.turn_id,
            false,
        )?;
        if !matches_binding(&current, frozen) || current.credential_ref != frozen.credential_ref {
            return Err(ServiceError::new(
                "CONNECTION_CHANGED",
                "恢复期间连接或凭证配置变化，未发送回执",
            ));
        }
        Ok(())
    }
}
