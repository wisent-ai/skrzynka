//! Contacts as a CSV table (RFC 4180), so a spreadsheet or a CRM can take
//! them in with every count and mark `message contacts` prints.

use super::{
    super::{tables::row, AppState},
    Contact, ContactFilter,
};
use crate::error::AppError;

/// The table's header, the field names `message contacts` prints.
const HEADER: &[&str] = &[
    "address",
    "name",
    "received",
    "last_received_at",
    "written",
    "last_written_at",
    "bounced",
    "suppressed",
    "blocked",
];

/// Every contact as one row under the header, in the order given: `name`
/// the first name the address was written with, or empty; instants empty
/// when there is none. Lines end in CRLF and a field holding a comma, a
/// quote or a line break is quoted, its quotes doubled, as RFC 4180 asks.
pub fn contact_table(contacts: &[Contact]) -> String {
    let header: Vec<String> = HEADER.iter().map(|field| field.to_string()).collect();
    let mut table = row(&header);
    for contact in contacts {
        let instant = |value: &Option<String>| match value {
            Some(written) => written.clone(),
            None => String::new(),
        };
        let name = match contact.names.first() {
            Some(name) => name.clone(),
            None => String::new(),
        };
        table.push_str(&row(&[
            contact.address.clone(),
            name,
            contact.received.to_string(),
            instant(&contact.last_received_at),
            contact.written.to_string(),
            instant(&contact.last_written_at),
            contact.bounced.to_string(),
            contact.suppressed.to_string(),
            contact.blocked.to_string(),
        ]));
    }
    table
}

impl AppState {
    /// The correspondents `list_contacts` answers for `filter`, as the CSV
    /// table `contact_table` writes.
    pub fn contacts_csv(&self, organization_id: &str, filter: &ContactFilter) -> Result<String, AppError> {
        Ok(contact_table(&self.list_contacts(organization_id, filter)?))
    }
}
