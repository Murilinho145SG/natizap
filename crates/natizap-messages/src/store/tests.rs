use super::*;
use crate::text_search::Case;
use std::path::PathBuf;

use natizap_protocol::{ContentKind, Extra, HdThumbnail, LinkPreview, Poll};

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
        extra: None,
        forwarding: None,
        link: None,
        secret: None,
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
fn keeps_the_link_a_file_was_uploaded_again_to() -> rusqlite::Result<()> {
    let mut store = MessageStore::in_memory()?;
    let chat = ChatId::new("1@lid");
    let mut sticker = message("1@lid", "s", 10, "");
    sticker.kind = ContentKind::Sticker;
    sticker.media = Some(Media {
        direct_path: "/v/old?oe=5F000000".into(),
        media_key: vec![1],
        ..Media::default()
    });
    store.apply(&[MessageChange::Received(sticker)])?;
    store.apply(&[MessageChange::MediaSaved {
        message_id: "s".into(),
        file: PathBuf::from("/tmp/s.webp"),
        direct_path: Some("/v/fresh".into()),
    }])?;

    let stored = store.latest(std::slice::from_ref(&chat), 1)?.remove(0);
    assert_eq!(
        stored.media.map(|media| media.direct_path).as_deref(),
        Some("/v/fresh")
    );
    assert_eq!(stored.media_file, Some(PathBuf::from("/tmp/s.webp")));
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
        direct_path: None,
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
        secret: None,
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
        secret: None,
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

#[test]
fn a_resent_copy_fills_in_the_media_the_first_lacked() -> rusqlite::Result<()> {
    let mut store = MessageStore::in_memory()?;
    let chat = ChatId::new("1@lid");
    let mut bare = message("1@lid", "s", 10, "first");
    bare.kind = ContentKind::Sticker;
    bare.text = None;
    let mut resent = bare.clone();
    resent.text = Some("second".into());
    resent.media = Some(Media {
        direct_path: "/v/sticker".into(),
        media_key: vec![4],
        ..Media::default()
    });
    store.apply(&[MessageChange::Received(bare)])?;
    store.apply(&[MessageChange::Received(resent)])?;

    let stored = store.latest(std::slice::from_ref(&chat), 1)?.remove(0);
    assert_eq!(stored.text, None);
    assert_eq!(
        stored.media.map(|media| media.direct_path),
        Some("/v/sticker".to_owned())
    );
    Ok(())
}

#[test]
fn a_resent_copy_fills_in_the_link_preview_the_first_lacked() -> rusqlite::Result<()> {
    let mut store = MessageStore::in_memory()?;
    let chat = ChatId::new("1@lid");
    let preview = LinkPreview {
        url: Some("https://natizap.dev".into()),
        title: Some("natizap".into()),
        description: Some("A WhatsApp client".into()),
        thumbnail: Some(vec![0xff, 0xd8]),
    };
    let mut with_preview = message("1@lid", "a", 10, "see https://natizap.dev");
    with_preview.link = Some(preview.clone());
    store.apply(&[
        MessageChange::Received(message("1@lid", "a", 10, "see https://natizap.dev")),
        MessageChange::Received(with_preview),
    ])?;

    let stored = store.library(std::slice::from_ref(&chat))?;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].link, Some(preview));
    Ok(())
}

#[test]
fn counts_how_far_back_a_message_lies() -> rusqlite::Result<()> {
    let mut store = MessageStore::in_memory()?;
    store.apply(&[
        MessageChange::Received(message("1@lid", "a", 10, "old")),
        MessageChange::Received(message("1@s.whatsapp.net", "b", 20, "middle")),
        MessageChange::Received(message("1@lid", "c", 30, "new")),
        MessageChange::Received(message("2@lid", "d", 40, "other chat")),
    ])?;
    let chats = [ChatId::new("1@lid"), ChatId::new("1@s.whatsapp.net")];
    assert_eq!(store.count_since(&chats, 20)?, 2);
    assert_eq!(store.count_since(&chats, 0)?, 3);
    Ok(())
}

#[test]
fn keeps_each_voters_latest_vote_on_a_poll() -> rusqlite::Result<()> {
    let mut store = MessageStore::in_memory()?;
    let mut poll = message("1@g.us", "poll", 10, "");
    poll.kind = ContentKind::Poll;
    poll.extra = Some(Extra::Poll(Poll {
        question: "Lunch?".into(),
        options: vec!["Pizza".into(), "Sushi".into()],
        multiple: false,
    }));
    poll.secret = Some(vec![7; 32]);
    let vote = |voter: Option<&str>, options: &[&str], cast_at| MessageChange::Voted {
        poll_id: "poll".into(),
        voter: voter.map(ChatId::new),
        options: options.iter().map(|&option| option.to_owned()).collect(),
        cast_at,
    };
    store.apply(&[
        MessageChange::Received(poll.clone()),
        vote(Some("2@lid"), &["Pizza"], 100),
        vote(Some("2@lid"), &["Sushi"], 200),
        // Delivered late: older than the vote it would replace.
        vote(Some("2@lid"), &["Pizza"], 150),
        vote(Some("3@lid"), &["Pizza"], 100),
        vote(Some("3@lid"), &[], 300),
        vote(None, &["Sushi"], 100),
    ])?;

    let mut votes = store.votes(&[poll])?.remove("poll").unwrap_or_default();
    votes.sort_by_key(|vote| vote.mine);
    assert_eq!(
        votes,
        [
            Vote {
                options: vec!["Sushi".into()],
                mine: false
            },
            Vote {
                options: vec!["Sushi".into()],
                mine: true
            },
        ]
    );
    Ok(())
}

fn ids(messages: &[Message]) -> Vec<&str> {
    messages.iter().map(|m| m.id.as_str()).collect()
}

#[test]
fn finds_text_mid_word_newest_first_in_one_chat_or_all() -> rusqlite::Result<()> {
    let mut store = MessageStore::in_memory()?;
    store.apply(&[
        MessageChange::Received(message("1@lid", "a", 10, "Finalizamos seu atendimento")),
        MessageChange::Received(message("1@lid", "b", 20, "quero ATENDIMENTO agora")),
        MessageChange::Received(message("2@lid", "c", 30, "outro atendimento")),
        MessageChange::Received(message("status@broadcast", "s", 40, "atendimento")),
        MessageChange::Received(message("1@lid", "d", 50, "nada a ver")),
    ])?;
    let one = [ChatId::new("1@lid")];
    assert_eq!(
        ids(&store.search(Some(&one), "tendim", Case::Insensitive, 10)?),
        ["b", "a"]
    );
    assert_eq!(
        ids(&store.search(None, "atendimento", Case::Insensitive, 10)?),
        ["c", "b", "a"]
    );
    assert_eq!(
        ids(&store.search(None, "atendimento", Case::Insensitive, 2)?),
        ["c", "b"]
    );
    Ok(())
}

#[test]
fn tells_capitals_apart_only_when_asked() -> rusqlite::Result<()> {
    let mut store = MessageStore::in_memory()?;
    store.apply(&[
        MessageChange::Received(message("1@lid", "a", 10, "Ação marcada")),
        MessageChange::Received(message("1@lid", "b", 20, "AÇÃO MARCADA")),
    ])?;
    assert_eq!(
        ids(&store.search(None, "ação", Case::Insensitive, 10)?),
        ["b", "a"]
    );
    assert_eq!(
        ids(&store.search(None, "Ação", Case::Sensitive, 10)?),
        ["a"]
    );
    // Shorter than the index reads, still folded beyond ASCII.
    assert_eq!(
        ids(&store.search(None, "çã", Case::Insensitive, 10)?),
        ["b", "a"]
    );
    assert_eq!(ids(&store.search(None, "ÇÃ", Case::Sensitive, 10)?), ["b"]);
    Ok(())
}

#[test]
fn forgets_old_text_once_edited_or_deleted() -> rusqlite::Result<()> {
    let mut store = MessageStore::in_memory()?;
    let chat = ChatId::new("1@lid");
    store.apply(&[
        MessageChange::Received(message("1@lid", "a", 10, "primeira versão")),
        MessageChange::Received(message("1@lid", "b", 20, "vai sumir")),
        MessageChange::Received(message("1@lid", "c", 30, "vai sair do aparelho")),
        MessageChange::Edited {
            chat: chat.clone(),
            id: "a".into(),
            text: Some("segunda versão".into()),
        },
        MessageChange::Deleted {
            chat: chat.clone(),
            id: "b".into(),
        },
        MessageChange::Removed {
            chat,
            id: "c".into(),
        },
    ])?;
    assert_eq!(
        ids(&store.search(None, "primeira", Case::Insensitive, 10)?),
        Vec::<&str>::new()
    );
    assert_eq!(
        ids(&store.search(None, "segunda", Case::Insensitive, 10)?),
        ["a"]
    );
    assert_eq!(
        ids(&store.search(None, "vai", Case::Insensitive, 10)?),
        Vec::<&str>::new()
    );
    assert_eq!(
        ids(&store.search(None, "   ", Case::Insensitive, 10)?),
        Vec::<&str>::new()
    );
    Ok(())
}
