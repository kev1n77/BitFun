//! Local static card registry.
//!
//! Each application release can add a new Markdown file under
//! `content/features/{locale}/` to register feature announcement cards.
//! Cards are loaded at startup and matched against the running version at
//! scheduling time, so old cards are automatically ignored once the user has
//! seen them.

use super::types::AnnouncementCard;

/// Returns locally registered announcement cards for the given locale.
///
/// Feature announcement cards are currently disabled.
pub fn local_cards(_locale: &str) -> Vec<AnnouncementCard> {
    Vec::new()
}
