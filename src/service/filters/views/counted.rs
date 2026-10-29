//! Saved views with how many stored messages each lists now.

use super::SavedView;
use crate::{error::AppError, service::AppState};
use serde::Serialize;

/// One view with how many stored messages it lists now, as `message view
/// list --count` prints it.
#[derive(Debug, Serialize)]
pub struct CountedView {
    #[serde(flatten)]
    pub view: SavedView,
    pub messages: usize,
}

impl AppState {
    /// The views `views` keeps, each with how many stored messages it lists
    /// now, counted exactly as `message list --view` lists them.
    pub fn counted_views(
        &self,
        organization_id: &str,
        contains: Option<&str>,
    ) -> Result<Vec<CountedView>, AppError> {
        self.views(organization_id, contains)?
            .into_iter()
            .map(|view| {
                let messages = self.list_messages(
                    organization_id,
                    view.filters.mailbox_id,
                    view.filters.filters(),
                    None,
                    None,
                )?;
                Ok(CountedView {
                    messages: messages.len(),
                    view,
                })
            })
            .collect()
    }
}
