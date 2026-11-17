use super::*;

fn message(chat: &str, id: &str, sent_at: i64, text: &str) -> Message {
    Message {
        chat: ChatId::new(chat),
        id: id.into(),
        from_me: false,
        sender: None,
        author: None,
        sent_at,
        kind: ContentKind::Text,
        text: Some(text.into()),
        edited: false,
        deleted: false,
        delivery: Delivery::Sent,
        quote: None,
        media: None,
        media_file: None,
        media_poster: None,
        starred: false,
        sender_phone: None,
    }
}

fn texts(messages: &[Message]) -> Vec<Option<&str>> {
    messages.iter().map(|m| m.text.as_deref()).collect()
}

#[test]
fn returns_the_newest_messages_of_every_address_oldest_first() -> rusqlite::Result<()> {
    let mut store = MessageStore::in_memory()?;
    store.apply(&[
        MessageChange::Received(message("1@lid", "a", 10, "old")),
        MessageChange::Received(message("1@s.whatsapp.net", "b", 20, "middle")),
        MessageChange::Received(message("1@lid", "c", 30, "new")),
        MessageChange::Received(message("2@lid", "d", 40, "other chat")),
    ])?;

    let chats = [ChatId::new("1@lid"), ChatId::new("1@s.whatsapp.net")];
    assert_eq!(
        texts(&store.latest(&chats, 2)?),
        [Some("middle"), Some("new")]
    );
    Ok(())
}

#[test]
fn keeps_the_first_copy_and_applies_edits_and_deletions() -> rusqlite::Result<()> {
    let mut store = MessageStore::in_memory()?;
    let chat = ChatId::new("1@lid");
    store.apply(&[
        MessageChange::Received(message("1@lid", "a", 10, "hello")),
        MessageChange::Received(message("1@lid", "a", 10, "duplicate")),
        MessageChange::Edited {
            chat: chat.clone(),
            id: "a".into(),
            text: Some("hi".into()),
        },
        MessageChange::Received(message("1@lid", "b", 20, "oops")),
        MessageChange::Deleted {
            chat: chat.clone(),
            id: "b".into(),
        },
    ])?;

    let messages = store.latest(&[chat], 10)?;
    assert_eq!(texts(&messages), [Some("hi"), None]);
    assert!(messages[0].edited && messages[1].deleted);
    Ok(())
}

#[test]
fn receipts_only_move_a_sent_message_forward() -> rusqlite::Result<()> {
    let mut store = MessageStore::in_memory()?;
    let chat = ChatId::new("1@lid");
    let mut sent = message("1@lid", "a", 10, "hi");
    sent.from_me = true;
    store.apply(&[MessageChange::Received(sent)])?;

    let progressed = |delivery| MessageChange::Progressed {
        ids: vec!["a".into()],
        delivery,
    };
    store.apply(&[progressed(Delivery::Read), progressed(Delivery::Delivered)])?;

    let delivery = store.latest(std::slice::from_ref(&chat), 1)?[0].delivery;
    assert_eq!(delivery, Delivery::Read);
    Ok(())
}

#[test]
fn keeps_one_reaction_per_person_and_drops_retracted_ones() -> rusqlite::Result<()> {
    let mut store = MessageStore::in_memory()?;
    let chat = ChatId::new("1@g.us");
    let react = |reactor: Option<&str>, emoji: Option<&str>| MessageChange::Reacted {
        chat: chat.clone(),
        message_id: "a".into(),
        reactor: reactor.map(ChatId::new),
        emoji: emoji.map(str::to_owned),
    };
    store.apply(&[
        MessageChange::Received(message("1@g.us", "a", 10, "hi")),
        react(Some("2@lid"), Some("👍")),
        react(Some("2@lid"), Some("❤️")),
        react(None, Some("😂")),
        react(Some("3@lid"), Some("😮")),
        react(Some("3@lid"), None),
    ])?;

    let messages = store.latest(std::slice::from_ref(&chat), 10)?;
    let mut reactions = store.reactions(&messages)?.remove("a").unwrap_or_default();
    reactions.sort_by(|a, b| a.emoji.cmp(&b.emoji));
    let summary: Vec<(&str, bool)> = reactions
        .iter()
        .map(|r| (r.emoji.as_str(), r.mine))
        .collect();
    assert_eq!(summary, [("❤️", false), ("😂", true)]);
    Ok(())
}

#[test]
fn keeps_media_details_and_where_the_file_was_saved() -> rusqlite::Result<()> {
    let mut store = MessageStore::in_memory()?;
    let chat = ChatId::new("1@lid");
    let mut photo = message("1@lid", "p", 10, "beach");
    photo.kind = ContentKind::Photo;
    photo.media = Some(Media {
        width: Some(800),
        thumbnail: Some(vec![0xff, 0xd8]),
        waveform: Some(vec![0, 50, 100]),
        hd_thumbnail: Some(HdThumbnail {
            direct_path: "/v/1-thumb".into(),
            sha256: vec![2],
            enc_sha256: vec![3],
        }),
        direct_path: "/v/1".into(),
        media_key: vec![1],
        size: Some(5_000_000_000),
        ..Media::default()
    });
    store.apply(&[MessageChange::Received(photo.clone())])?;
    store.apply(&[MessageChange::MediaSaved {
        message_id: "p".into(),
        file: PathBuf::from("/tmp/p.jpg"),
    }])?;
    store.apply(&[MessageChange::PosterSaved {
        message_id: "p".into(),
        file: PathBuf::from("/tmp/p-poster.jpg"),
    }])?;

    let stored = store.latest(std::slice::from_ref(&chat), 1)?.remove(0);
    assert_eq!(stored.media, photo.media);
    assert_eq!(stored.media_file, Some(PathBuf::from("/tmp/p.jpg")));
    assert_eq!(
        stored.media_poster,
        Some(PathBuf::from("/tmp/p-poster.jpg"))
    );
    Ok(())
}

#[test]
fn a_sent_message_takes_its_real_id_once_accepted() -> rusqlite::Result<()> {
    let mut store = MessageStore::in_memory()?;
    let chat = ChatId::new("1@lid");
    let mut outgoing = message("1@lid", "local-1", 10, "hey");
    outgoing.from_me = true;
    outgoing.delivery = Delivery::Sending;
    store.apply(&[MessageChange::Sending(outgoing)])?;
    store.apply(&[MessageChange::Accepted {
        chat: chat.clone(),
        local_id: "local-1".into(),
        id: "3EB0".into(),
        media: None,
    }])?;

    let messages = store.latest(std::slice::from_ref(&chat), 10)?;
    assert_eq!(
        (messages[0].id.as_str(), messages[0].delivery),
        ("3EB0", Delivery::Sent)
    );
    assert_eq!(store.latest_incoming(&[chat], 5)?, []);
    Ok(())
}

#[test]
fn a_sent_file_records_where_it_was_uploaded() -> rusqlite::Result<()> {
    let mut store = MessageStore::in_memory()?;
    let chat = ChatId::new("1@lid");
    let mut outgoing = message("1@lid", "local-2", 10, "");
    outgoing.from_me = true;
    outgoing.kind = ContentKind::Photo;
    outgoing.delivery = Delivery::Sending;
    outgoing.media = Some(Media {
        width: Some(400),
        thumbnail: Some(vec![0xff, 0xd8]),
        ..Media::default()
    });
    store.apply(&[MessageChange::Sending(outgoing)])?;
    let uploaded = Media {
        direct_path: "/v/up".into(),
        media_key: vec![9],
        file_sha256: vec![8],
        file_enc_sha256: vec![7],
        size: Some(1234),
        ..Media::default()
    };
    store.apply(&[MessageChange::Accepted {
        chat: chat.clone(),
        local_id: "local-2".into(),
        id: "3EB1".into(),
        media: Some(uploaded),
    }])?;

    let stored = store.latest(std::slice::from_ref(&chat), 1)?.remove(0);
    let media = stored.media.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    assert_eq!(
        (
            media.direct_path.as_str(),
            media.media_key,
            media.size,
            media.width
        ),
        ("/v/up", vec![9], Some(1234), Some(400))
    );
    Ok(())
}

#[test]
fn shows_a_pin_until_it_runs_out_or_is_undone() -> rusqlite::Result<()> {
    let mut store = MessageStore::in_memory()?;
    let chat = ChatId::new("1@lid");
    store.apply(&[MessageChange::Received(message("1@lid", "a", 10, "plan"))])?;
    let pinned_id = |store: &MessageStore, now| {
        store
            .pinned(std::slice::from_ref(&chat), now)
            .map(|pinned| pinned.map(|message| message.id))
    };
    store.apply(&[MessageChange::Pinned {
        message_id: "a".into(),
        until: Some(1_000),
    }])?;
    assert_eq!(pinned_id(&store, 500)?.as_deref(), Some("a"));
    assert_eq!(pinned_id(&store, 1_500)?, None);
    store.apply(&[MessageChange::Pinned {
        message_id: "a".into(),
        until: None,
    }])?;
    assert_eq!(pinned_id(&store, 500)?, None);
    Ok(())
}

#[test]
fn counts_a_chats_media_and_starred_messages() -> rusqlite::Result<()> {
    let mut store = MessageStore::in_memory()?;
    let chat = ChatId::new("1@lid");
    let mut photo = message("1@lid", "p", 10, "");
    photo.kind = ContentKind::Photo;
    let mut video = message("1@lid", "v", 20, "");
    video.kind = ContentKind::Video;
    let mut document = message("1@lid", "d", 30, "");
    document.kind = ContentKind::Document;
    store.apply(&[
        MessageChange::Received(photo),
        MessageChange::Received(video),
        MessageChange::Received(document),
        MessageChange::Received(message("1@lid", "t", 40, "hi")),
        MessageChange::Starred {
            message_id: "t".into(),
            starred: true,
        },
    ])?;
    let (count, recent) = store.media(std::slice::from_ref(&chat), 4)?;
    let ids: Vec<&str> = recent.iter().map(|message| message.id.as_str()).collect();
    assert_eq!((count, ids), (3, vec!["v", "p"]));
    assert_eq!(store.starred_count(std::slice::from_ref(&chat))?, 1);
    Ok(())
}

#[test]
fn keeps_which_messages_are_starred() -> rusqlite::Result<()> {
    let mut store = MessageStore::in_memory()?;
    let chat = ChatId::new("1@lid");
    store.apply(&[MessageChange::Received(message("1@lid", "a", 10, "keep"))])?;
    store.apply(&[MessageChange::Starred {
        message_id: "a".into(),
        starred: true,
    }])?;
    assert!(store.latest(std::slice::from_ref(&chat), 1)?[0].starred);
    store.apply(&[MessageChange::Starred {
        message_id: "a".into(),
        starred: false,
    }])?;
    assert!(!store.latest(std::slice::from_ref(&chat), 1)?[0].starred);
    Ok(())
}
