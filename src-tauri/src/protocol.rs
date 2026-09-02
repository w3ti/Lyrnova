use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalKind {
    Command,
    FileWrite,
    Network,
    ExternalPath,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Accept,
    AcceptForSession,
    Decline,
    CancelTurn,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    ThreadStarted {
        event_id: String,
        thread_id: String,
        project_id: String,
    },
    TurnStarted {
        event_id: String,
        thread_id: String,
        turn_id: String,
    },
    MessageDelta {
        event_id: String,
        turn_id: String,
        item_id: String,
        delta: String,
    },
    ToolStarted {
        event_id: String,
        turn_id: String,
        tool_id: String,
        title: String,
    },
    ApprovalRequested {
        event_id: String,
        turn_id: String,
        approval_id: String,
        kind: ApprovalKind,
        summary: String,
    },
    ApprovalResolved {
        event_id: String,
        approval_id: String,
        decision: ApprovalDecision,
    },
    PatchProposed {
        event_id: String,
        turn_id: String,
        patch_id: String,
        files: Vec<String>,
    },
    TurnCompleted {
        event_id: String,
        turn_id: String,
    },
    TurnFailed {
        event_id: String,
        turn_id: String,
        message: String,
        retryable: bool,
    },
}

impl AgentEvent {
    fn event_id(&self) -> &str {
        match self {
            Self::ThreadStarted { event_id, .. }
            | Self::TurnStarted { event_id, .. }
            | Self::MessageDelta { event_id, .. }
            | Self::ToolStarted { event_id, .. }
            | Self::ApprovalRequested { event_id, .. }
            | Self::ApprovalResolved { event_id, .. }
            | Self::PatchProposed { event_id, .. }
            | Self::TurnCompleted { event_id, .. }
            | Self::TurnFailed { event_id, .. } => event_id,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TranscriptItem {
    AssistantMessage {
        turn_id: String,
        item_id: String,
        text: String,
    },
    Tool {
        turn_id: String,
        tool_id: String,
        title: String,
    },
    Patch {
        turn_id: String,
        patch_id: String,
        files: Vec<String>,
    },
    Error {
        turn_id: String,
        message: String,
        retryable: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingApproval {
    pub turn_id: String,
    pub kind: ApprovalKind,
    pub summary: String,
}

#[derive(Debug, Default)]
pub struct SessionState {
    pub project_id: Option<String>,
    pub thread_id: Option<String>,
    pub active_turn_id: Option<String>,
    pub transcript: Vec<TranscriptItem>,
    pub pending_approvals: BTreeMap<String, PendingApproval>,
    seen_events: HashSet<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplyOutcome {
    Applied,
    Duplicate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProtocolError {
    ThreadMismatch,
    TurnMismatch,
    ApprovalNotFound,
}

impl SessionState {
    pub fn apply(&mut self, event: AgentEvent) -> Result<ApplyOutcome, ProtocolError> {
        if self.seen_events.contains(event.event_id()) {
            return Ok(ApplyOutcome::Duplicate);
        }

        self.validate(&event)?;
        let event_id = event.event_id().to_owned();

        match event {
            AgentEvent::ThreadStarted {
                thread_id,
                project_id,
                ..
            } => {
                self.thread_id = Some(thread_id);
                self.project_id = Some(project_id);
            }
            AgentEvent::TurnStarted { turn_id, .. } => {
                self.active_turn_id = Some(turn_id);
            }
            AgentEvent::MessageDelta {
                turn_id,
                item_id,
                delta,
                ..
            } => {
                if let Some(TranscriptItem::AssistantMessage { text, .. }) = self
                    .transcript
                    .iter_mut()
                    .find(|item| matches!(item, TranscriptItem::AssistantMessage { item_id: existing, .. } if existing == &item_id))
                {
                    text.push_str(&delta);
                } else {
                    self.transcript.push(TranscriptItem::AssistantMessage {
                        turn_id,
                        item_id,
                        text: delta,
                    });
                }
            }
            AgentEvent::ToolStarted {
                turn_id,
                tool_id,
                title,
                ..
            } => self.transcript.push(TranscriptItem::Tool {
                turn_id,
                tool_id,
                title,
            }),
            AgentEvent::ApprovalRequested {
                turn_id,
                approval_id,
                kind,
                summary,
                ..
            } => {
                self.pending_approvals.insert(
                    approval_id,
                    PendingApproval {
                        turn_id,
                        kind,
                        summary,
                    },
                );
            }
            AgentEvent::ApprovalResolved { approval_id, .. } => {
                self.pending_approvals.remove(&approval_id);
            }
            AgentEvent::PatchProposed {
                turn_id,
                patch_id,
                files,
                ..
            } => self.transcript.push(TranscriptItem::Patch {
                turn_id,
                patch_id,
                files,
            }),
            AgentEvent::TurnCompleted { .. } => {
                self.active_turn_id = None;
                self.pending_approvals.clear();
            }
            AgentEvent::TurnFailed {
                turn_id,
                message,
                retryable,
                ..
            } => {
                self.transcript.push(TranscriptItem::Error {
                    turn_id,
                    message,
                    retryable,
                });
                self.active_turn_id = None;
                self.pending_approvals.clear();
            }
        }

        self.seen_events.insert(event_id);
        Ok(ApplyOutcome::Applied)
    }

    fn validate(&self, event: &AgentEvent) -> Result<(), ProtocolError> {
        match event {
            AgentEvent::ThreadStarted { .. } => Ok(()),
            AgentEvent::TurnStarted { thread_id, .. } => {
                if self.thread_id.as_deref() == Some(thread_id) {
                    Ok(())
                } else {
                    Err(ProtocolError::ThreadMismatch)
                }
            }
            AgentEvent::MessageDelta { turn_id, .. }
            | AgentEvent::ToolStarted { turn_id, .. }
            | AgentEvent::ApprovalRequested { turn_id, .. }
            | AgentEvent::PatchProposed { turn_id, .. }
            | AgentEvent::TurnCompleted { turn_id, .. }
            | AgentEvent::TurnFailed { turn_id, .. } => {
                if self.active_turn_id.as_deref() == Some(turn_id) {
                    Ok(())
                } else {
                    Err(ProtocolError::TurnMismatch)
                }
            }
            AgentEvent::ApprovalResolved { approval_id, .. } => {
                if self.pending_approvals.contains_key(approval_id) {
                    Ok(())
                } else {
                    Err(ProtocolError::ApprovalNotFound)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start_session() -> SessionState {
        let mut state = SessionState::default();
        state
            .apply(AgentEvent::ThreadStarted {
                event_id: "event-1".into(),
                thread_id: "thread-1".into(),
                project_id: "project-1".into(),
            })
            .unwrap();
        state
            .apply(AgentEvent::TurnStarted {
                event_id: "event-2".into(),
                thread_id: "thread-1".into(),
                turn_id: "turn-1".into(),
            })
            .unwrap();
        state
    }

    #[test]
    fn rebuilds_streamed_message_without_duplicates() {
        let mut state = start_session();
        let first = AgentEvent::MessageDelta {
            event_id: "event-3".into(),
            turn_id: "turn-1".into(),
            item_id: "message-1".into(),
            delta: "Olá".into(),
        };

        assert_eq!(state.apply(first.clone()).unwrap(), ApplyOutcome::Applied);
        assert_eq!(state.apply(first).unwrap(), ApplyOutcome::Duplicate);
        state
            .apply(AgentEvent::MessageDelta {
                event_id: "event-4".into(),
                turn_id: "turn-1".into(),
                item_id: "message-1".into(),
                delta: ", mundo".into(),
            })
            .unwrap();

        assert_eq!(
            state.transcript,
            [TranscriptItem::AssistantMessage {
                turn_id: "turn-1".into(),
                item_id: "message-1".into(),
                text: "Olá, mundo".into(),
            }]
        );
    }

    #[test]
    fn rejects_events_for_another_thread_or_turn() {
        let mut state = start_session();
        assert_eq!(
            state.apply(AgentEvent::TurnStarted {
                event_id: "event-x".into(),
                thread_id: "thread-x".into(),
                turn_id: "turn-x".into(),
            }),
            Err(ProtocolError::ThreadMismatch)
        );
        assert_eq!(
            state.apply(AgentEvent::ToolStarted {
                event_id: "event-y".into(),
                turn_id: "turn-x".into(),
                tool_id: "tool-1".into(),
                title: "Ler arquivo".into(),
            }),
            Err(ProtocolError::TurnMismatch)
        );
    }

    #[test]
    fn approval_must_exist_before_it_is_resolved() {
        let mut state = start_session();
        assert_eq!(
            state.apply(AgentEvent::ApprovalResolved {
                event_id: "event-3".into(),
                approval_id: "approval-missing".into(),
                decision: ApprovalDecision::Decline,
            }),
            Err(ProtocolError::ApprovalNotFound)
        );

        state
            .apply(AgentEvent::ApprovalRequested {
                event_id: "event-4".into(),
                turn_id: "turn-1".into(),
                approval_id: "approval-1".into(),
                kind: ApprovalKind::Command,
                summary: "Executar cargo test".into(),
            })
            .unwrap();
        state
            .apply(AgentEvent::ApprovalResolved {
                event_id: "event-5".into(),
                approval_id: "approval-1".into(),
                decision: ApprovalDecision::Accept,
            })
            .unwrap();

        assert!(state.pending_approvals.is_empty());
    }

    #[test]
    fn completion_clears_turn_and_pending_approvals() {
        let mut state = start_session();
        state
            .apply(AgentEvent::ApprovalRequested {
                event_id: "event-3".into(),
                turn_id: "turn-1".into(),
                approval_id: "approval-1".into(),
                kind: ApprovalKind::Network,
                summary: "Acessar registry".into(),
            })
            .unwrap();
        state
            .apply(AgentEvent::TurnCompleted {
                event_id: "event-4".into(),
                turn_id: "turn-1".into(),
            })
            .unwrap();

        assert_eq!(state.active_turn_id, None);
        assert!(state.pending_approvals.is_empty());
    }

    #[test]
    fn protocol_round_trip_preserves_event() {
        let event = AgentEvent::PatchProposed {
            event_id: "event-1".into(),
            turn_id: "turn-1".into(),
            patch_id: "patch-1".into(),
            files: vec!["src/main.rs".into(), "README.md".into()],
        };

        let json = serde_json::to_string(&event).unwrap();
        let decoded: AgentEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, event);
    }
}
