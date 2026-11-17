//! A business's opening hours, as its profile shows them for today.

use chrono::{Datelike as _, Local, NaiveDateTime, Timelike as _};
use natizap_protocol::{DayHours, Hours};

/// Whether a business is open, and its hours today.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Today {
    /// Open right now, when its hours say: by appointment says nothing.
    pub(crate) open: Option<bool>,
    /// Today's hours, e.g. "Open 24 hours" or "08:00 – 18:00".
    pub(crate) hours: String,
}

/// Today's hours in `week`, or nothing when the business gives none. The
/// business's own time zone is not applied: it is nearly always the
/// user's.
pub(crate) fn today(week: &[DayHours]) -> Option<Today> {
    today_at(week, Local::now().naive_local())
}

fn today_at(week: &[DayHours], now: NaiveDateTime) -> Option<Today> {
    if week.is_empty() {
        return None;
    }
    let weekday = u8::try_from(now.weekday().num_days_from_monday()).ok()?;
    let minute = now.hour() * 60 + now.minute();
    let Some(day) = week.iter().find(|day| day.weekday == weekday) else {
        return Some(Today {
            open: Some(false),
            hours: "Closed today".to_owned(),
        });
    };
    Some(match day.hours {
        Hours::AllDay => Today {
            open: Some(true),
            hours: "Open 24 hours".to_owned(),
        },
        Hours::ByAppointment => Today {
            open: None,
            hours: "By appointment only".to_owned(),
        },
        Hours::Between(opens, closes) => Today {
            // Hours past midnight close the next day.
            open: Some(if opens <= closes {
                (opens..closes).contains(&minute)
            } else {
                minute >= opens || minute < closes
            }),
            hours: format!("{} – {}", clock(opens), clock(closes)),
        },
    })
}

fn clock(minutes: u32) -> String {
    format!("{:02}:{:02}", minutes / 60 % 24, minutes % 60)
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    fn at(day: u32, hour: u32) -> NaiveDateTime {
        // 2026-11-16 is a Monday.
        NaiveDate::from_ymd_opt(2026, 11, day)
            .and_then(|date| date.and_hms_opt(hour, 30, 0))
            .unwrap_or_default()
    }

    #[test]
    fn says_whether_a_business_is_open_now() {
        let week = [
            DayHours {
                weekday: 0,
                hours: Hours::Between(8 * 60, 18 * 60),
            },
            DayHours {
                weekday: 1,
                hours: Hours::Between(22 * 60, 2 * 60),
            },
        ];
        let today = |day, hour| today_at(&week, at(day, hour));
        assert_eq!(
            today(16, 9),
            Some(Today {
                open: Some(true),
                hours: "08:00 – 18:00".to_owned()
            })
        );
        assert_eq!(today(16, 19).and_then(|today| today.open), Some(false));
        assert_eq!(today(17, 23).and_then(|today| today.open), Some(true));
        assert_eq!(
            today(18, 9).map(|today| today.hours),
            Some("Closed today".to_owned())
        );
        assert_eq!(today_at(&[], at(16, 9)), None);
    }
}
