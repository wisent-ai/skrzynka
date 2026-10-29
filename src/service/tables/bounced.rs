//! The addresses that bounced for good as a table: what `message bounced
//! --csv` writes, for a CRM or a mailing tool to stop writing to them.

use super::{super::delivery::BouncedAddress, header, optional, row};

/// The table's header, the field names `message bounced` prints.
const HEADER: &[&str] = &[
    "address",
    "reports",
    "last_report_id",
    "last_reported_at",
    "status",
    "diagnostic",
];

/// Every bounced address as one row under the header, in the order given;
/// a status or diagnostic the newest report did not state is empty.
pub fn bounced_table(addresses: &[BouncedAddress]) -> String {
    let mut table = header(HEADER);
    for bounced in addresses {
        table.push_str(&row(&[
            bounced.address.clone(),
            bounced.reports.to_string(),
            bounced.last_report_id.to_string(),
            bounced.last_reported_at.clone(),
            optional(&bounced.status),
            optional(&bounced.diagnostic),
        ]));
    }
    table
}
