//! A business account's public profile, read with a query of our own: the
//! library's leaves out the verification level, which decides the seal, and
//! the cover photo.

use std::sync::Arc;

use whatsapp_rust::anyhow;
use whatsapp_rust::http::HttpRequest;
use whatsapp_rust::prelude::Client;
use whatsapp_rust::wacore::iq::business::{
    BusinessHourMode, BusinessProfile, BusinessProfileSpec, DayOfWeek,
};
use whatsapp_rust::wacore::iq::spec::IqSpec;
use whatsapp_rust::wacore::request::InfoQuery;
use whatsapp_rust::wacore_binary::NodeContent;
use whatsapp_rust::wacore_binary::NodeRef;
use whatsapp_rust::{Jid, NodeBuilder, Server};

/// A business account's public details.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Business {
    /// The name WhatsApp verified for it, shown instead of a number.
    pub name: Option<String>,
    /// WhatsApp vouches for it: it shows the seal.
    pub verified: bool,
    /// What kind of business it is, e.g. "Restaurant".
    pub category: Option<String>,
    /// How it describes itself.
    pub description: Option<String>,
    /// Where it is.
    pub address: Option<String>,
    /// How to write to it.
    pub email: Option<String>,
    /// Its websites.
    pub websites: Vec<String>,
    /// When it opens, by day; empty when it does not say.
    pub hours: Vec<DayHours>,
    /// The banner above its picture, as image bytes.
    pub cover: Option<Vec<u8>>,
}

/// When a business opens on one day of the week.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayHours {
    /// Days from Monday, 0 to 6.
    pub weekday: u8,
    /// Its hours that day.
    pub hours: Hours,
}

/// A business's hours on one day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hours {
    /// Open all day.
    AllDay,
    /// Open between these minutes after midnight.
    Between(u32, u32),
    /// Only by appointment.
    ByAppointment,
}

/// The query WhatsApp Web makes: a newer version of the library's, whose
/// answer also carries the cover photo.
struct FullProfile {
    jid: Jid,
}

/// What the library's parser leaves out of the answer.
struct Extras {
    verified: bool,
    cover_url: Option<String>,
}

impl IqSpec for FullProfile {
    type Response = Option<(BusinessProfile, Extras)>;

    fn build_iq(&self) -> InfoQuery<'static> {
        InfoQuery::get(
            "w:biz",
            Jid::new("", Server::Pn),
            Some(NodeContent::Nodes(vec![
                NodeBuilder::new("business_profile")
                    .attr("v", "372")
                    .children([NodeBuilder::new("profile").attr("jid", &self.jid).build()])
                    .build(),
            ])),
        )
    }

    fn parse_response(&self, response: &NodeRef<'_>) -> anyhow::Result<Self::Response> {
        let Some(profile) = BusinessProfileSpec::new(&self.jid).parse_response(response)? else {
            return Ok(None);
        };
        let node = response
            .get_optional_child("business_profile")
            .and_then(|business| business.get_optional_child("profile"));
        let verified = node
            .and_then(|node| node.get_optional_child("biz_identity_info"))
            .and_then(|identity| identity.attrs().optional_string("vlevel"))
            .is_some_and(|level| level == "high");
        let cover_url = node
            .and_then(|node| node.get_optional_child("cover_photo"))
            .and_then(|cover| {
                cover
                    .attrs()
                    .optional_string("url")
                    .map(std::borrow::Cow::into_owned)
                    .or_else(|| {
                        cover
                            .get_optional_child("url")
                            .and_then(NodeRef::content_as_string)
                            .map(|url| url.to_string())
                    })
                    // What WhatsApp sends today: `<cover_photo id="…">`
                    // holding the link itself.
                    .or_else(|| {
                        cover
                            .content_as_string()
                            .map(|url| url.trim().to_owned())
                            .filter(|url| url.starts_with("https://"))
                    })
            });
        if let Some(node) = node {
            let tags: Vec<String> = node
                .children()
                .unwrap_or_default()
                .iter()
                .map(|child| child.tag.to_string())
                .collect();
            tracing::debug!(?tags, "business profile fields");
        }
        Ok(Some((
            profile,
            Extras {
                verified,
                cover_url,
            },
        )))
    }
}

/// The business profile of `jid`, a business named `name` by its verified
/// name; one WhatsApp gives no details of shows only that name.
pub(crate) async fn business(
    client: &Arc<Client>,
    jid: &Jid,
    name: Option<String>,
) -> anyhow::Result<Business> {
    Ok(business_if_any(client, jid, name.clone())
        .await?
        .unwrap_or(Business {
            name,
            ..Business::default()
        }))
}

/// The business profile of `jid`, named `name`, or `None` when it is not
/// a business. Asks the business service only, not the contacts service
/// whose rate limit a burst of lookups soon reaches.
pub(crate) async fn business_if_any(
    client: &Arc<Client>,
    jid: &Jid,
    name: Option<String>,
) -> anyhow::Result<Option<Business>> {
    let answer = match client.execute(FullProfile { jid: jid.clone() }).await {
        Ok(answer) => answer,
        Err(error) => {
            // An older query still answers, without the newer fields.
            tracing::debug!(%error, "full business profile refused");
            client
                .execute(BusinessProfileSpec::new(jid))
                .await?
                .map(|profile| {
                    (
                        profile,
                        Extras {
                            verified: false,
                            cover_url: None,
                        },
                    )
                })
        }
    };
    let Some((details, extras)) = answer else {
        return Ok(None);
    };
    let cover = match extras.cover_url {
        Some(url) => download(client, url).await,
        None => None,
    };
    let non_empty = |text: String| Some(text).filter(|text| !text.trim().is_empty());
    Ok(Some(Business {
        name,
        verified: extras.verified,
        category: details
            .categories
            .first()
            .map(|category| category.name.clone()),
        description: non_empty(details.description),
        address: details.address,
        email: details.email,
        websites: details.website,
        hours: details
            .business_hours
            .business_config
            .unwrap_or_default()
            .iter()
            .filter_map(|day| {
                let hours = match (&day.mode, day.open_time, day.close_time) {
                    (BusinessHourMode::Open24H, _, _) => Hours::AllDay,
                    (BusinessHourMode::AppointmentOnly, _, _) => Hours::ByAppointment,
                    (BusinessHourMode::SpecificHours, Some(open), Some(close)) => {
                        Hours::Between(open, close)
                    }
                    _ => return None,
                };
                Some(DayHours {
                    weekday: weekday(&day.day_of_week)?,
                    hours,
                })
            })
            .collect(),
        cover,
    }))
}

fn weekday(day: &DayOfWeek) -> Option<u8> {
    Some(match day {
        DayOfWeek::Monday => 0,
        DayOfWeek::Tuesday => 1,
        DayOfWeek::Wednesday => 2,
        DayOfWeek::Thursday => 3,
        DayOfWeek::Friday => 4,
        DayOfWeek::Saturday => 5,
        DayOfWeek::Sunday => 6,
        DayOfWeek::Other(_) => return None,
    })
}

async fn download(client: &Arc<Client>, url: String) -> Option<Vec<u8>> {
    match client.http_client.execute(HttpRequest::get(url)).await {
        Ok(response) if (200..300).contains(&response.status_code) => Some(response.body),
        Ok(response) => {
            tracing::debug!(status = response.status_code, "cover photo refused");
            None
        }
        Err(error) => {
            tracing::debug!(%error, "cover photo unavailable");
            None
        }
    }
}
