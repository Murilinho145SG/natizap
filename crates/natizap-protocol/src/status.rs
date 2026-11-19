//! Posting status updates, which go to the people `audience` names, as
//! WhatsApp's status privacy "My contacts" does.

use gpui::{App, Task};
use whatsapp_rust::prelude::wa;
use whatsapp_rust::{Jid, StatusSendOptions};

use crate::outgoing::{self, Outgoing};
use crate::{ActionError, ChatId, Protocol, actions};

impl Protocol {
    /// Posts `text` as a status on a `background` colour (0xAARRGGBB),
    /// seen by `audience`.
    pub fn post_status_text(
        &self,
        text: String,
        background: u32,
        audience: &[ChatId],
        cx: &App,
    ) -> Task<Result<String, ActionError>> {
        let audience = match jids(audience) {
            Ok(audience) => audience,
            Err(error) => return Task::ready(Err(error)),
        };
        self.connection.run(cx, move |client| async move {
            client
                .status()
                .send_text(
                    &text,
                    background,
                    wa::message::extended_text_message::FontType::SYSTEM,
                    &audience,
                    StatusSendOptions::default(),
                )
                .await
                .map(|sent| sent.message_id.into_string())
                .map_err(|error| ActionError::Rejected(error.to_string()))
        })
    }

    /// Posts a photo or video, with its caption, as a status seen by
    /// `audience`.
    pub fn post_status_media(
        &self,
        mut post: Outgoing,
        audience: &[ChatId],
        cx: &App,
    ) -> Task<Result<String, ActionError>> {
        let audience = match jids(audience) {
            Ok(audience) => audience,
            Err(error) => return Task::ready(Err(error)),
        };
        self.connection.run(cx, move |client| async move {
            let (kind, caption) = (post.kind, post.caption.take());
            let (media, fresh) = outgoing::upload(&client, post).await?;
            let message = outgoing::media_message(kind, &media, caption, Some(fresh));
            client
                .status()
                .send_raw(message, &audience, StatusSendOptions::default())
                .await
                .map(|sent| sent.message_id.into_string())
                .map_err(|error| ActionError::Rejected(error.to_string()))
        })
    }
}

fn jids(chats: &[ChatId]) -> Result<Vec<Jid>, ActionError> {
    chats.iter().map(actions::jid).collect()
}
