//! How a business's public details read on a profile: labeled fields,
//! links the system opens, and its hours.

use gpui::prelude::*;
use gpui::{Div, div, px, rgb};
use natizap_protocol::Business;
use natizap_ui::theme::Theme;

use crate::hours;

/// The business's description, address, email and websites, each that it
/// gives.
pub(crate) fn business_fields(business: &Business, theme: Theme) -> Vec<Div> {
    let fields = [
        ("Description", business.description.clone()),
        ("Address", business.address.clone()),
    ];
    let email = business
        .email
        .clone()
        .map(|email| links("Email", vec![(format!("mailto:{email}"), email)], theme));
    let websites: Vec<(String, String)> = business
        .websites
        .iter()
        .map(|site| {
            let target = if site.contains("://") {
                site.clone()
            } else {
                format!("https://{site}")
            };
            (target, site.clone())
        })
        .collect();
    let websites = (!websites.is_empty()).then(|| links("Website", websites, theme));
    fields
        .into_iter()
        .filter_map(|(label, value)| Some(field(label, value?, theme)))
        .chain(email)
        .chain(websites)
        .collect()
}

/// Everything the business gives, its category and every day's hours
/// included, as its own profile lists it.
pub(crate) fn business_info(business: &Business, theme: Theme) -> Div {
    let category = business
        .category
        .clone()
        .map(|category| field("Category", category, theme));
    let week = hours::week(&business.hours);
    let hours = (!week.is_empty()).then(|| {
        labeled("Business hours", theme).children(week.into_iter().map(|(day, hours)| {
            div()
                .flex()
                .text_size(px(14.5))
                .child(div().w(px(120.0)).flex_none().child(day))
                .child(div().text_color(rgb(theme.muted)).child(hours))
        }))
    });
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_4()
        .children(category)
        .children(business_fields(business, theme))
        .children(hours)
}

pub(crate) fn field(label: &'static str, value: String, theme: Theme) -> Div {
    labeled(label, theme).child(div().text_size(px(14.5)).child(value))
}

fn labeled(label: &'static str, theme: Theme) -> Div {
    div().flex().flex_col().gap_1().child(
        div()
            .text_size(px(12.5))
            .text_color(rgb(theme.muted))
            .child(label),
    )
}

/// A field of `(target, text)` links, each opened by the system.
fn links(label: &'static str, links: Vec<(String, String)>, theme: Theme) -> Div {
    labeled(label, theme).children(
        links
            .into_iter()
            .enumerate()
            .map(|(index, (target, text))| {
                div()
                    .id((label, index))
                    .text_size(px(14.5))
                    .text_color(rgb(theme.link))
                    .cursor_pointer()
                    .hover(Styled::underline)
                    .on_click(move |_, _, cx| cx.open_url(&target))
                    .child(text)
            }),
    )
}
