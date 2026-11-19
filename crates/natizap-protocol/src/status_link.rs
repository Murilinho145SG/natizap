//! Where a status leads: the app or page it was shared from, such as a
//! song on Spotify, or the channel a post was reshared from.

use whatsapp_rust::prelude::wa;

use crate::content::context_info;

/// What a status update points to, as the phone offers it on the status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLink {
    /// What the button says, such as "Open in Spotify".
    pub label: String,
    /// Where it leads, when it leads outside WhatsApp.
    pub url: Option<String>,
}

impl StatusLink {
    /// Where `message`, a status update, leads, if anywhere.
    #[must_use]
    pub fn of(message: &wa::Message) -> Option<Self> {
        use wa::__buffa::oneof::status_attribution::AttributionData;
        let context = context_info(message)?;
        let attributed = context.status_attributions.iter().find_map(|attribution| {
            let data = attribution.attribution_data.as_ref();
            match data {
                Some(AttributionData::ExternalShare(share)) => {
                    let url = share
                        .action_url
                        .clone()
                        .or_else(|| attribution.action_url.clone())?;
                    let label = share
                        .source
                        .and_then(app_name)
                        .map_or_else(|| "Open link".to_owned(), |app| format!("Open in {app}"));
                    Some(Self {
                        label,
                        url: Some(url),
                    })
                }
                Some(AttributionData::Music(music)) => {
                    let song = [music.title.as_deref(), music.author.as_deref()]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(" · ");
                    (!song.is_empty()).then(|| Self {
                        label: format!("♪ {song}"),
                        url: attribution.action_url.clone(),
                    })
                }
                _ => None,
            }
        });
        attributed.or_else(|| {
            let channel = context.forwarded_newsletter_message_info.as_option()?;
            let name = channel.newsletter_name.clone()?;
            Some(Self {
                label: format!("From the channel {name}"),
                url: None,
            })
        })
    }
}

/// The app a status was shared from, by name.
fn app_name(source: wa::status_attribution::external_share::Source) -> Option<&'static str> {
    use wa::status_attribution::external_share::Source;
    Some(match source {
        Source::INSTAGRAM => "Instagram",
        Source::FACEBOOK => "Facebook",
        Source::MESSENGER => "Messenger",
        Source::SPOTIFY => "Spotify",
        Source::YOUTUBE => "YouTube",
        Source::PINTEREST => "Pinterest",
        Source::THREADS => "Threads",
        Source::APPLE_MUSIC => "Apple Music",
        Source::SHARECHAT => "ShareChat",
        Source::GOOGLE_PHOTOS => "Google Photos",
        Source::SOUNDCLOUD => "SoundCloud",
        Source::SHAZAM => "Shazam",
        Source::UNKNOWN => return None,
    })
}

#[cfg(test)]
mod tests {
    use whatsapp_rust::prelude::MessageField;
    use whatsapp_rust::prelude::wa::__buffa::oneof::status_attribution::AttributionData;
    use whatsapp_rust::prelude::wa::status_attribution::ExternalShare;
    use whatsapp_rust::prelude::wa::status_attribution::external_share::Source;

    use super::*;

    fn photo(context: wa::ContextInfo) -> wa::Message {
        wa::Message {
            image_message: MessageField::some(wa::message::ImageMessage {
                context_info: MessageField::some(context),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn a_song_shared_from_spotify_opens_in_spotify() {
        let message = photo(wa::ContextInfo {
            status_attributions: vec![wa::StatusAttribution {
                attribution_data: Some(AttributionData::ExternalShare(Box::new(ExternalShare {
                    action_url: Some("https://open.spotify.com/track/1".into()),
                    source: Some(Source::SPOTIFY),
                    ..Default::default()
                }))),
                ..Default::default()
            }],
            ..Default::default()
        });
        assert_eq!(
            StatusLink::of(&message),
            Some(StatusLink {
                label: "Open in Spotify".into(),
                url: Some("https://open.spotify.com/track/1".into()),
            })
        );
    }

    #[test]
    fn a_plain_photo_leads_nowhere() {
        assert_eq!(StatusLink::of(&photo(wa::ContextInfo::default())), None);
    }
}
