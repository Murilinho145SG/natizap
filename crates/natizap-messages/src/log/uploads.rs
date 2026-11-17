//! Sending files the user picked.

use std::path::PathBuf;

use gpui::{AppContext as _, Context};
use natizap_playback::{Heard, save_recording};
use natizap_protocol::{ChatId, ContentKind, Media, Outgoing, Reply};

use super::sending::outgoing_message;
use super::{Input, MessageLog, MessageSent};
use crate::media::Fetch;
use crate::message::MessageChange;
use crate::outbox::prepare;

impl MessageLog {
    /// Sends each file at `paths` to the open chat as a message of its own,
    /// the first carrying `caption` and replying to `reply`, as the phone
    /// does. Each shows in the chat as soon as it is read, before WhatsApp
    /// has it.
    pub(crate) fn send_files(
        &mut self,
        paths: Vec<PathBuf>,
        mut caption: Option<String>,
        mut reply: Option<Reply>,
        as_document: bool,
        cx: &mut Context<Self>,
    ) {
        for path in paths {
            let Some((chat, local_id, sent_at)) = self.next_outgoing() else {
                return;
            };
            let (caption, reply) = (caption.take(), reply.take());
            let media_dir = self.media_dir.clone();
            let id = local_id.clone();
            cx.spawn(async move |this, cx| {
                let prepared = cx
                    .background_spawn(async move {
                        let outgoing = prepare(&path, caption, as_document)
                            .map_err(|error| error.to_string())?;
                        // The bubble shows and plays this copy, the very
                        // bytes sent, from the start.
                        let kept = Fetch {
                            message_id: id,
                            kind: outgoing.kind,
                            media: describe(&outgoing),
                        }
                        .path_in(&media_dir);
                        std::fs::create_dir_all(&media_dir).map_err(|error| error.to_string())?;
                        std::fs::write(&kept, &outgoing.bytes)
                            .map_err(|error| error.to_string())?;
                        Ok::<_, String>((outgoing, kept))
                    })
                    .await;
                match prepared {
                    Ok((outgoing, kept)) => {
                        let _ = this.update(cx, |log, cx| {
                            log.send_prepared(chat, local_id, sent_at, reply, outgoing, kept, cx);
                        });
                    }
                    Err(error) => tracing::warn!(%error, "could not send a file"),
                }
            })
            .detach();
        }
    }

    /// Sends what the microphone heard as a voice note to the open chat,
    /// replying to `reply`. It is encoded off the UI thread.
    pub(crate) fn send_voice(
        &mut self,
        heard: Heard,
        reply: Option<Reply>,
        cx: &mut Context<Self>,
    ) {
        let Some((chat, local_id, sent_at)) = self.next_outgoing() else {
            return;
        };
        let media_dir = self.media_dir.clone();
        let id = local_id.clone();
        cx.spawn(async move |this, cx| {
            let prepared = cx
                .background_spawn(async move {
                    std::fs::create_dir_all(&media_dir).map_err(|error| error.to_string())?;
                    let recorded = media_dir.join(format!("{id}.ogg"));
                    let recording =
                        save_recording(&heard, &recorded).map_err(|error| error.to_string())?;
                    let bytes = std::fs::read(&recorded).map_err(|error| error.to_string())?;
                    let outgoing = Outgoing {
                        kind: ContentKind::VoiceMessage,
                        bytes,
                        mime: "audio/ogg; codecs=opus".to_owned(),
                        file_name: None,
                        caption: None,
                        thumbnail: None,
                        width: None,
                        height: None,
                        seconds: Some(recording.seconds),
                        waveform: Some(recording.waveform),
                    };
                    Ok::<_, String>((outgoing, recorded))
                })
                .await;
            match prepared {
                Ok((outgoing, kept)) => {
                    let _ = this.update(cx, |log, cx| {
                        log.send_prepared(chat, local_id, sent_at, reply, outgoing, kept, cx);
                    });
                }
                Err(error) => tracing::warn!(%error, "could not send a voice note"),
            }
        })
        .detach();
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "one message's parts, gathered once by send_files"
    )]
    fn send_prepared(
        &mut self,
        chat: ChatId,
        local_id: String,
        sent_at: i64,
        reply: Option<Reply>,
        outgoing: Outgoing,
        kept: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let mut message = outgoing_message(&chat, &local_id, sent_at, reply.as_ref());
        message.kind = outgoing.kind;
        message.text.clone_from(&outgoing.caption);
        message.media = Some(describe(&outgoing));
        message.media_file = Some(kept);
        self.send_input(Input::from(MessageChange::Sending(message)));
        cx.emit(MessageSent {
            chat: chat.clone(),
            text: outgoing
                .caption
                .clone()
                .unwrap_or_else(|| outgoing.kind.label().to_owned()),
            sent_at,
        });

        let sending = self
            .protocol
            .read(cx)
            .send_media(&chat, outgoing, reply, cx);
        let inputs = self.inputs.clone();
        cx.background_spawn(async move {
            let change = match sending.await {
                Ok(sent) => MessageChange::Accepted {
                    chat,
                    local_id,
                    id: sent.id,
                    media: Some(sent.media),
                },
                Err(error) => {
                    tracing::warn!(%error, "file not sent");
                    MessageChange::Rejected { chat, local_id }
                }
            };
            // Only fails while shutting down, when nothing is left to update.
            let _ = inputs.send(Input::from(change)).await;
        })
        .detach();
    }
}

/// What the bubble shows of a file being sent, before WhatsApp says where it
/// lives.
fn describe(outgoing: &Outgoing) -> Media {
    Media {
        mime: Some(outgoing.mime.clone()),
        file_name: outgoing.file_name.clone(),
        size: u64::try_from(outgoing.bytes.len()).ok(),
        width: outgoing.width,
        height: outgoing.height,
        seconds: outgoing.seconds,
        thumbnail: outgoing.thumbnail.clone(),
        waveform: outgoing.waveform.clone(),
        ..Media::default()
    }
}
