use natizap_protocol::PersonName;

use super::*;

fn at(day: u32, hour: u32, minute: u32) -> i64 {
    Local
        .with_ymd_and_hms(2026, 11, day, hour, minute, 0)
        .single()
        .map_or(0, |time| time.timestamp())
}

fn message(sent_at: i64, from_me: bool, sender: Option<&str>) -> Message {
    Message {
        chat: ChatId::new("1@g.us"),
        id: sent_at.to_string(),
        from_me,
        sender: sender.map(ChatId::new),
        author: sender.map(|_| "Ana".to_owned()),
        sent_at,
        kind: ContentKind::Text,
        text: Some("hi".into()),
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

fn layout(items: &[Item]) -> Vec<String> {
    items
        .iter()
        .map(|item| match item {
            Item::Day(label) => format!("[{label}]"),
            Item::Bubble(bubble) => {
                let side = if bubble.from_me { "me" } else { "them" };
                let burst = if bubble.starts_burst { "+" } else { "" };
                let author = bubble
                    .author
                    .as_ref()
                    .map_or("", |author| author.name.as_ref());
                format!("{burst}{side}{author}")
            }
        })
        .collect()
}

#[test]
fn names_a_member_as_the_address_book_or_they_did() {
    let mut saved = message(1_000, false, Some("309@lid"));
    saved.author = Some("brunao".into());
    let mut chosen = message(9_000, false, Some("410@lid"));
    chosen.author = None;
    let phone = ChatId::new("5511912345678@s.whatsapp.net");
    let members = Members {
        phones: HashMap::from([(ChatId::new("410@lid"), phone.clone())]),
        names: Arc::new(HashMap::from([
            (
                ChatId::new("309@lid"),
                PersonName {
                    saved: Some("Bruno Lima".into()),
                    chosen: None,
                },
            ),
            (
                phone,
                PersonName {
                    saved: None,
                    chosen: Some("Carla".into()),
                },
            ),
        ])),
        ..Members::default()
    };
    let names: Vec<String> = items(
        &[saved, chosen],
        &Addons {
            reactions: &HashMap::new(),
            votes: &HashMap::new(),
        },
        "",
        &members,
        Local::now(),
    )
    .into_iter()
    .filter_map(|item| match item {
        Item::Bubble(bubble) => bubble.sender.map(|sender| sender.name.to_string()),
        Item::Day(_) => None,
    })
    .collect();
    assert_eq!(names, ["Bruno Lima", "~ Carla"]);
}

#[test]
fn names_a_member_from_their_other_messages_and_the_groups_numbers() {
    let lid = ChatId::new("208@lid");
    let mut named = message(1_000, false, Some("208@lid"));
    named.author = Some("Beatriz".into());
    let mut unnamed = message(5_000, false, Some("208@lid"));
    unnamed.author = None;
    let members = Members {
        community_admins: vec![ChatId::new("5511976543210@s.whatsapp.net")],
        phones: HashMap::from([(lid, ChatId::new("5511976543210@s.whatsapp.net"))]),
        ..Members::default()
    };
    let now = Local::now();
    let authors: Vec<(String, Option<String>, Option<String>)> = items(
        &[named, unnamed],
        &Addons {
            reactions: &HashMap::new(),
            votes: &HashMap::new(),
        },
        "",
        &members,
        now,
    )
    .into_iter()
    .filter_map(|item| match item {
        Item::Bubble(bubble) => bubble.sender,
        Item::Day(_) => None,
    })
    .map(|author| {
        (
            author.name.to_string(),
            author.phone.map(|phone| phone.to_string()),
            author.role.map(|role| role.to_string()),
        )
    })
    .collect();
    let expected = (
        "~ Beatriz".to_owned(),
        Some("+55 11 97654-3210".to_owned()),
        Some("Community admin".to_owned()),
    );
    assert_eq!(authors, [expected.clone(), expected]);
}

#[test]
fn separates_days_and_groups_bursts_by_sender_and_time() {
    let Some(now) = Local.with_ymd_and_hms(2026, 11, 16, 18, 0, 0).single() else {
        panic!("test date must exist in the local zone");
    };
    let ana = Some("5511@s.whatsapp.net");
    let messages = [
        message(at(15, 9, 0), false, ana),
        message(at(16, 10, 0), false, ana),
        message(at(16, 10, 1), false, ana),
        message(at(16, 10, 2), true, None),
        message(at(16, 10, 30), true, None),
    ];

    assert_eq!(
        layout(&items(
            &messages,
            &Addons {
                reactions: &HashMap::new(),
                votes: &HashMap::new(),
            },
            "Ana",
            &Members::default(),
            now
        )),
        [
            "[Yesterday]",
            "+them~ Ana",
            "[Today]",
            "+them~ Ana",
            "them",
            "+me",
            "+me"
        ]
    );
}

/// A photo sent `seconds` after 10:00 on the 16th, with `caption`.
fn photo(seconds: i64, caption: Option<&str>) -> Message {
    let mut photo = message(at(16, 10, 0) + seconds, true, None);
    photo.kind = ContentKind::Photo;
    photo.text = caption.map(str::to_owned);
    photo.media = Some(natizap_protocol::Media::default());
    photo
}

/// Each bubble as how many pictures it shows, and its caption.
fn pictures(messages: &[Message]) -> Vec<(usize, Option<String>)> {
    let Some(now) = Local.with_ymd_and_hms(2026, 11, 16, 18, 0, 0).single() else {
        panic!("test date must exist in the local zone");
    };
    let addons = Addons {
        reactions: &HashMap::new(),
        votes: &HashMap::new(),
    };
    items(messages, &addons, "Ana", &Members::default(), now)
        .iter()
        .filter_map(|item| match item {
            Item::Bubble(bubble) => Some(bubble),
            Item::Day(_) => None,
        })
        .map(|bubble| {
            let caption = match &bubble.body {
                Body::Content { text, .. } => text.as_ref().map(|text| text.text.to_string()),
                _ => None,
            };
            (
                usize::from(bubble.media.is_some()) + bubble.album.len(),
                caption,
            )
        })
        .collect()
}

#[test]
fn folds_four_or_more_pictures_sent_together_into_one_album() {
    let burst: Vec<Message> = (0..5)
        .map(|index| photo(index * 2, (index == 4).then_some("trip")))
        .collect();
    assert_eq!(pictures(&burst), [(5, Some("trip".to_owned()))]);

    let few: Vec<Message> = (0..3).map(|index| photo(index, None)).collect();
    assert_eq!(pictures(&few), [(1, None), (1, None), (1, None)]);
}

#[test]
fn a_caption_or_a_pause_ends_an_album() {
    let messages = [
        photo(0, None),
        photo(1, Some("first")),
        photo(2, None),
        photo(3, None),
        photo(4, None),
        photo(5, None),
        // Over a minute later: a picture of its own.
        photo(200, None),
    ];
    assert_eq!(
        pictures(&messages),
        [
            (1, None),
            (1, Some("first".to_owned())),
            (4, None),
            (1, None)
        ]
    );
}
