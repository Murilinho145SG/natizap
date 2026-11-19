//! Status updates as natizap keeps them: each post, and the posts of one
//! person gathered as their status shows.

use std::path::PathBuf;

use natizap_protocol::{ChatId, ContentKind, Media};

/// How long a status lasts, in seconds.
pub(crate) const DAY: i64 = 24 * 60 * 60;

/// One status update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Post {
    /// Who posted it; this account for its own.
    pub poster: ChatId,
    /// WhatsApp's id for it.
    pub id: String,
    /// When it was posted, in Unix seconds.
    pub sent_at: i64,
    /// A text, a photo or a video.
    pub kind: ContentKind,
    /// The text, or a photo's or video's caption.
    pub text: Option<String>,
    /// A text status's background, 0xAARRGGBB.
    pub background: Option<u32>,
    /// The name the poster goes by, as their phone sent it.
    pub author: Option<String>,
    /// The photo or video, to download.
    pub media: Option<Media>,
    /// The downloaded photo or video.
    pub file: Option<PathBuf>,
    /// This account saw it.
    pub seen: bool,
    /// This account posted it.
    pub mine: bool,
}

impl Post {
    /// One line saying what it is, as a reply to it quotes it.
    #[must_use]
    pub fn summary(&self) -> String {
        match (&self.text, self.kind) {
            (Some(text), _) if !text.trim().is_empty() => text.clone(),
            (_, kind) => kind.label().to_owned(),
        }
    }
}

/// One person's posts of the last day, oldest first, as their status
/// shows them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Poster {
    pub poster: ChatId,
    pub posts: Vec<Post>,
}

impl Poster {
    /// Whether any of the posts is still unseen.
    #[must_use]
    pub fn unseen(&self) -> bool {
        self.posts.iter().any(|post| !post.seen)
    }

    /// When the newest post went up.
    #[must_use]
    pub fn latest(&self) -> i64 {
        self.posts
            .iter()
            .map(|post| post.sent_at)
            .max()
            .unwrap_or(0)
    }

    /// The first post not seen yet, where opening the status starts; the
    /// first post when all were seen.
    #[must_use]
    pub fn start(&self) -> usize {
        self.posts.iter().position(|post| !post.seen).unwrap_or(0)
    }
}

/// `posts` from the day before `now`, gathered by poster, newest poster
/// first; this account's own posts apart.
pub(crate) fn gather(posts: &[Post], now: i64) -> (Option<Poster>, Vec<Poster>) {
    let mut mine: Vec<Post> = Vec::new();
    let mut others: Vec<Poster> = Vec::new();
    for post in posts.iter().filter(|post| now - post.sent_at < DAY) {
        if post.mine {
            mine.push(post.clone());
        } else if let Some(poster) = others.iter_mut().find(|p| p.poster == post.poster) {
            poster.posts.push(post.clone());
        } else {
            others.push(Poster {
                poster: post.poster.clone(),
                posts: vec![post.clone()],
            });
        }
    }
    for poster in &mut others {
        poster.posts.sort_by_key(|post| post.sent_at);
    }
    others.sort_by_key(|poster| std::cmp::Reverse(poster.latest()));
    mine.sort_by_key(|post| post.sent_at);
    let mine = (!mine.is_empty()).then(|| Poster {
        poster: mine[0].poster.clone(),
        posts: mine,
    });
    (mine, others)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(poster: &str, id: &str, sent_at: i64, seen: bool) -> Post {
        Post {
            poster: ChatId::new(poster),
            id: id.into(),
            sent_at,
            kind: ContentKind::Text,
            text: Some(id.into()),
            background: None,
            author: None,
            media: None,
            file: None,
            seen,
            mine: poster == "me",
        }
    }

    #[test]
    fn gathers_the_last_day_by_poster_newest_first() {
        let now = 10 * DAY;
        let posts = [
            post("a@lid", "a1", now - 100, true),
            post("b@lid", "b1", now - 50, false),
            post("a@lid", "a2", now - 10, false),
            post("c@lid", "old", now - DAY - 1, false),
            post("me", "m1", now - 5, false),
        ];
        let (mine, others) = gather(&posts, now);
        assert_eq!(mine.map(|mine| mine.posts.len()), Some(1));
        let order: Vec<&str> = others.iter().map(|p| p.poster.as_str()).collect();
        assert_eq!(order, ["a@lid", "b@lid"]);
        let a: Vec<&str> = others[0].posts.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(a, ["a1", "a2"]);
    }

    #[test]
    fn opens_on_the_first_unseen_post() {
        let poster = Poster {
            poster: ChatId::new("a@lid"),
            posts: vec![post("a@lid", "1", 1, true), post("a@lid", "2", 2, false)],
        };
        assert!(poster.unseen());
        assert_eq!(poster.start(), 1);
        let seen = Poster {
            posts: vec![post("a@lid", "1", 1, true)],
            ..poster
        };
        assert!(!seen.unseen());
        assert_eq!(seen.start(), 0);
    }
}
