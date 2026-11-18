//! Messages as the store reads them back from its rows.

use std::path::PathBuf;

use natizap_protocol::{ChatId, ContentKind, Forwarding, HdThumbnail, LinkPreview, Media, Quote};
use rusqlite::Row;

use super::extra;
use crate::message::{Delivery, Message};

fn media_from_row(row: &Row<'_>) -> rusqlite::Result<Option<Media>> {
    let Some(direct_path) = row.get::<_, Option<String>>("media_direct_path")? else {
        return Ok(None);
    };
    Ok(Some(Media {
        mime: row.get("media_mime")?,
        file_name: row.get("media_name")?,
        size: row
            .get::<_, Option<i64>>("media_size")?
            .and_then(|size| u64::try_from(size).ok()),
        width: row.get("media_width")?,
        height: row.get("media_height")?,
        seconds: row.get("media_seconds")?,
        thumbnail: row.get("media_thumbnail")?,
        waveform: row.get("media_waveform")?,
        pages: row.get("media_pages")?,
        hd_thumbnail: hd_thumbnail_from_row(row)?,
        direct_path,
        media_key: row
            .get::<_, Option<Vec<u8>>>("media_key")?
            .unwrap_or_default(),
        file_sha256: row
            .get::<_, Option<Vec<u8>>>("media_sha256")?
            .unwrap_or_default(),
        file_enc_sha256: row
            .get::<_, Option<Vec<u8>>>("media_enc_sha256")?
            .unwrap_or_default(),
    }))
}

fn hd_thumbnail_from_row(row: &Row<'_>) -> rusqlite::Result<Option<HdThumbnail>> {
    let Some(direct_path) = row.get::<_, Option<String>>("media_hd_thumbnail_path")? else {
        return Ok(None);
    };
    Ok(Some(HdThumbnail {
        direct_path,
        sha256: row
            .get::<_, Option<Vec<u8>>>("media_hd_thumbnail_sha256")?
            .unwrap_or_default(),
        enc_sha256: row
            .get::<_, Option<Vec<u8>>>("media_hd_thumbnail_enc_sha256")?
            .unwrap_or_default(),
    }))
}

fn quote_from_row(row: &Row<'_>) -> rusqlite::Result<Option<Quote>> {
    let Some(id) = row.get::<_, Option<String>>("quoted_id")? else {
        return Ok(None);
    };
    let sender: Option<String> = row.get("quoted_sender")?;
    Ok(Some(Quote {
        id,
        sender: sender.map(|jid| ChatId::new(&jid)),
        summary: row.get("quoted_summary")?,
    }))
}

pub(super) fn message_from_row(row: &Row<'_>) -> rusqlite::Result<Message> {
    let kind: String = row.get("kind")?;
    let delivery: String = row.get("delivery")?;
    Ok(Message {
        chat: ChatId::new(&row.get::<_, String>("chat")?),
        id: row.get("id")?,
        from_me: row.get("from_me")?,
        sender: row
            .get::<_, Option<String>>("sender")?
            .map(|jid| ChatId::new(&jid)),
        author: row.get("author")?,
        sent_at: row.get("sent_at")?,
        kind: ContentKind::from_label(&kind).unwrap_or(ContentKind::Text),
        text: row.get("text")?,
        edited: row.get("edited")?,
        deleted: row.get("deleted")?,
        delivery: Delivery::from_name(&delivery).unwrap_or(Delivery::Sent),
        quote: quote_from_row(row)?,
        media: media_from_row(row)?,
        media_file: row
            .get::<_, Option<String>>("media_file")?
            .map(PathBuf::from),
        media_poster: row
            .get::<_, Option<String>>("media_poster")?
            .map(PathBuf::from),
        starred: row.get("starred")?,
        sender_phone: row
            .get::<_, Option<String>>("sender_phone")?
            .map(|phone| ChatId::new(&phone)),
        extra: row
            .get::<_, Option<String>>("extra")?
            .as_deref()
            .and_then(extra::decode),
        forwarding: row.get::<_, Option<u8>>("forwarding")?.and_then(
            |forwarding| match forwarding {
                1 => Some(Forwarding::Once),
                2 => Some(Forwarding::Often),
                _ => None,
            },
        ),
        link: link_from_row(row)?,
    })
}

/// The link preview kept with a message, when it has one.
fn link_from_row(row: &Row<'_>) -> rusqlite::Result<Option<LinkPreview>> {
    let preview = LinkPreview {
        url: row.get("link_url")?,
        title: row.get("link_title")?,
        description: row.get("link_description")?,
        thumbnail: row.get("link_thumbnail")?,
    };
    Ok((preview.title.is_some() || preview.thumbnail.is_some()).then_some(preview))
}
