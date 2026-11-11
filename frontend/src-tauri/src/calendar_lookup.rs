//! Name an auto-started recording after the calendar event in progress. Reads every
//! calendar macOS Calendar syncs (Exchange included) through EventKit, gated by the
//! `calendar_meeting_name` preference; access is requested when the setting is turned on.

/// One calendar event reduced to what the pick needs. Times are Unix seconds.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub title: String,
    pub start: f64,
    pub end: f64,
    pub all_day: bool,
    /// Canceled, or declined by the current user.
    pub skipped: bool,
    /// A Zoom, Teams, Meet, Webex or FaceTime link in the location, notes or URL.
    pub has_link: bool,
}

/// How early a meeting may start relative to the call (joining a few minutes early).
pub const EARLY_JOIN_SECONDS: f64 = 5.0 * 60.0;

/// Hosts that mark an event as a call. Matched as substrings, case-insensitively.
pub const MEETING_LINK_HOSTS: [&str; 5] = ["zoom.us", "teams.microsoft.com", "meet.google.com", "webex.com", "facetime.apple.com"];

pub fn has_meeting_link(haystack: &str) -> bool {
    let h = haystack.to_lowercase();
    MEETING_LINK_HOSTS.iter().any(|host| h.contains(host))
}

/// The event the call most likely belongs to: running now or starting within
/// `EARLY_JOIN_SECONDS`, not all-day, not skipped. A meeting link wins, then the latest start.
pub fn pick_title(now: f64, events: &[Candidate]) -> Option<String> {
    events
        .iter()
        .filter(|e| !e.all_day && !e.skipped && e.end > now && e.start <= now + EARLY_JOIN_SECONDS)
        .filter(|e| !e.title.trim().is_empty())
        .max_by(|a, b| (a.has_link, a.start).partial_cmp(&(b.has_link, b.start)).unwrap_or(std::cmp::Ordering::Equal))
        .map(|e| e.title.trim().to_string())
}

#[cfg(target_os = "macos")]
mod mac {
    use super::Candidate;
    use objc2::rc::Retained;
    use objc2::runtime::Bool;
    use objc2_event_kit::{EKAuthorizationStatus, EKEntityType, EKEventStatus, EKEventStore, EKParticipantStatus};
    use objc2_foundation::{NSDate, NSError};

    pub fn has_access() -> bool {
        let status = unsafe { EKEventStore::authorizationStatusForEntityType(EKEntityType::Event) };
        status == EKAuthorizationStatus::FullAccess
    }

    /// No answer stored for this build yet. TCC keys the grant on the code signature, so a
    /// grant given to a dev or ad-hoc build does not carry over to the notarized one.
    pub fn undetermined() -> bool {
        let status = unsafe { EKEventStore::authorizationStatusForEntityType(EKEntityType::Event) };
        status == EKAuthorizationStatus::NotDetermined
    }

    /// Shows the macOS prompt the first time; later calls return the stored answer.
    /// ponytail: full access API only (macOS 14+); add requestAccessToEntityType if macOS 13 matters.
    pub fn request_access() -> bool {
        if has_access() {
            return true;
        }
        let store = unsafe { EKEventStore::new() };
        let (tx, rx) = std::sync::mpsc::channel();
        let block = block2::RcBlock::new(move |granted: Bool, _err: *mut NSError| {
            let _ = tx.send(granted.as_bool());
        });
        unsafe { store.requestFullAccessToEventsWithCompletion(&*block as *const _ as *mut _) };
        rx.recv_timeout(std::time::Duration::from_secs(120)).unwrap_or(false)
    }

    fn text(s: Option<Retained<objc2_foundation::NSString>>) -> String {
        s.map(|s| s.to_string()).unwrap_or_default()
    }

    pub fn events_now() -> Vec<Candidate> {
        if !has_access() {
            log::warn!("Calendar lookup skipped: no full Calendar access (System Settings > Privacy & Security > Calendars)");
            return Vec::new();
        }
        unsafe {
            let store = EKEventStore::new();
            let from = NSDate::dateWithTimeIntervalSinceNow(0.0);
            let to = NSDate::dateWithTimeIntervalSinceNow(super::EARLY_JOIN_SECONDS);
            let predicate = store.predicateForEventsWithStartDate_endDate_calendars(&from, &to, None);
            let events = store.eventsMatchingPredicate(&predicate);
            events
                .iter()
                .map(|e| {
                    let declined = e.attendees().map_or(false, |ps| {
                        ps.iter().any(|p| p.isCurrentUser() && p.participantStatus() == EKParticipantStatus::Declined)
                    });
                    let haystack = format!(
                        "{} {} {}",
                        text(e.location()),
                        text(e.notes()),
                        e.URL().and_then(|u| u.absoluteString()).map(|s| s.to_string()).unwrap_or_default()
                    );
                    Candidate {
                        title: text(Some(e.title())),
                        start: e.startDate().timeIntervalSince1970(),
                        end: e.endDate().timeIntervalSince1970(),
                        all_day: e.isAllDay(),
                        skipped: declined || e.status() == EKEventStatus::Canceled,
                        has_link: super::has_meeting_link(&haystack),
                    }
                })
                .collect()
        }
    }
}

/// Title of the meeting in progress, or None without access or a matching event.
pub fn current_meeting_title() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs_f64();
        pick_title(now, &mac::events_now())
    }
    #[cfg(not(target_os = "macos"))]
    None
}

/// At launch, when calendar naming is on but this build was never answered: show the prompt
/// now rather than leave the lookup silently empty at the next call.
pub fn request_access_if_undetermined() {
    #[cfg(target_os = "macos")]
    if mac::undetermined() {
        log::info!("Calendar naming is on but access was never asked for this build; prompting");
        let granted = mac::request_access();
        log::info!("Calendar access granted: {}", granted);
    }
}

/// Asks macOS for calendar access (prompt on first use). False means denied:
/// the user has to allow Noesis in System Settings > Privacy & Security > Calendars.
#[tauri::command]
pub async fn calendar_request_access() -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    return tokio::task::spawn_blocking(mac::request_access).await.map_err(|e| e.to_string());
    #[cfg(not(target_os = "macos"))]
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(title: &str, start: f64, end: f64) -> Candidate {
        Candidate { title: title.into(), start, end, all_day: false, skipped: false, has_link: false }
    }

    #[test]
    fn running_and_early_join_events_match_but_past_and_later_ones_do_not() {
        let now = 10_000.0;
        assert_eq!(pick_title(now, &[ev("Running", now - 600.0, now + 600.0)]).as_deref(), Some("Running"));
        assert_eq!(pick_title(now, &[ev("Soon", now + 240.0, now + 3600.0)]).as_deref(), Some("Soon"));
        assert_eq!(pick_title(now, &[ev("Later", now + 900.0, now + 3600.0)]), None);
        assert_eq!(pick_title(now, &[ev("Over", now - 3600.0, now)]), None);
    }

    #[test]
    fn all_day_skipped_and_blank_events_are_ignored() {
        let now = 10_000.0;
        let mut all_day = ev("Holiday", now - 600.0, now + 600.0);
        all_day.all_day = true;
        let mut declined = ev("Declined", now - 600.0, now + 600.0);
        declined.skipped = true;
        assert_eq!(pick_title(now, &[all_day, declined, ev("  ", now - 60.0, now + 60.0)]), None);
    }

    #[test]
    fn a_meeting_link_wins_then_latest_start() {
        let now = 10_000.0;
        let mut linked = ev("Teams one", now - 1800.0, now + 600.0);
        linked.has_link = true;
        let later = ev("Later start", now - 60.0, now + 600.0);
        assert_eq!(pick_title(now, &[linked.clone(), later.clone()]).as_deref(), Some("Teams one"));
        assert_eq!(
            pick_title(now, &[ev("Early", now - 1800.0, now + 600.0), later]).as_deref(),
            Some("Later start")
        );
    }

    #[test]
    fn meeting_links_of_the_usual_apps_are_recognised() {
        for h in ["https://zoom.us/j/1", "https://teams.microsoft.com/l/meetup-join/x", "https://meet.google.com/abc-defg-hij", "https://bcg.webex.com/meet/x", "https://facetime.apple.com/join#v=1"] {
            assert!(has_meeting_link(&format!("Agenda {} notes", h)), "{}", h);
        }
        assert!(!has_meeting_link("Room 4.12, bring the deck"));
    }
}
