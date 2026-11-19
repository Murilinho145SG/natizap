//! Calls in their chats: every call the account made or that rang, filed
//! as a message where it happened, as the phone shows them: which way it
//! went, voice or video, and how it ended or how long it lasted.

use natizap_protocol::{CallDetails, CallOutcome, CallRecord, ChatId, ContentKind, Extra};

use super::MessageLog;
use crate::message::{Delivery, Message, MessageChange};

impl MessageLog {
    /// Files `calls` in their chats. A call already filed, under its id,
    /// stays as it was.
    pub fn file_calls(&self, calls: &[CallRecord]) {
        for call in calls {
            self.send_input(super::Input::from(MessageChange::Received(call_message(
                call,
            ))));
        }
    }
}

/// `call` as a message in its chat: the group's, for a group call.
pub(crate) fn call_message(call: &CallRecord) -> Message {
    let chat = call.group.clone().unwrap_or_else(|| call.peer.clone());
    message(
        chat,
        call.id.clone(),
        // In a group's chat, who called; one to one, the chat says it.
        call.group.as_ref().map(|_| call.peer.clone()),
        call.at,
        call.video,
        CallDetails {
            incoming: call.incoming,
            outcome: call.outcome,
            seconds: call.seconds,
        },
    )
}

/// A call that rang here and nobody answered, as a message in its chat.
pub(crate) fn missed_message(
    chat: ChatId,
    id: String,
    caller: Option<ChatId>,
    at: i64,
    video: bool,
) -> Message {
    let details = CallDetails {
        incoming: true,
        outcome: CallOutcome::Missed,
        seconds: None,
    };
    message(chat, id, caller, at, video, details)
}

fn message(
    chat: ChatId,
    id: String,
    sender: Option<ChatId>,
    at: i64,
    video: bool,
    details: CallDetails,
) -> Message {
    Message {
        chat,
        // The call's id: a call heard of twice is filed once.
        id,
        // On the side of whoever called, as the phone shows it.
        from_me: !details.incoming,
        sender,
        author: None,
        sent_at: at,
        kind: if video {
            ContentKind::VideoCall
        } else {
            ContentKind::VoiceCall
        },
        text: None,
        quote: None,
        media: None,
        extra: Some(Extra::Call(details)),
        forwarding: None,
        link: None,
        secret: None,
        media_file: None,
        media_poster: None,
        starred: false,
        sender_phone: None,
        edited: false,
        deleted: false,
        delivery: Delivery::Sent,
    }
}

#[cfg(test)]
mod tests {
    use natizap_protocol::{CallOutcome, CallRecord, ChatId, ContentKind, Extra};

    use super::call_message;

    fn call(group: Option<&str>, incoming: bool) -> CallRecord {
        CallRecord {
            id: "CALL1".into(),
            peer: ChatId::new("5511912345678@s.whatsapp.net"),
            group: group.map(ChatId::new),
            incoming,
            video: true,
            outcome: CallOutcome::Answered,
            at: 1_700_000_000,
            seconds: Some(31),
        }
    }

    #[test]
    fn a_call_is_filed_in_its_chat_on_the_callers_side() {
        let made = call_message(&call(None, false));
        assert_eq!(made.chat, ChatId::new("5511912345678@s.whatsapp.net"));
        assert!(made.from_me);
        assert_eq!(made.kind, ContentKind::VideoCall);
        let Some(Extra::Call(details)) = made.extra else {
            panic!("no call details");
        };
        assert_eq!(details.seconds, Some(31));
        let came = call_message(&call(None, true));
        assert!(!came.from_me);
    }

    #[test]
    fn a_group_call_is_filed_in_the_group_under_its_caller() {
        let filed = call_message(&call(Some("123@g.us"), true));
        assert_eq!(filed.chat, ChatId::new("123@g.us"));
        assert_eq!(
            filed.sender,
            Some(ChatId::new("5511912345678@s.whatsapp.net"))
        );
    }
}
