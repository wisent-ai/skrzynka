//! Recorded files as a table: what `message attachment find --csv` writes,
//! to audit in a spreadsheet which mail carried which file and what takes the
//! room.

use super::{
    super::tables::{header, optional, row},
    StoredFile,
};

/// The table's header, the field names `message attachment find` prints.
const HEADER: &[&str] = &[
    "message_id", "mailbox_id", "sender", "subject", "received_at", "index", "filename", "content_type", "size_bytes",
];

/// Every file as one row under the header, in the order given; a name,
/// subject or size the import did not record is empty.
pub fn file_table(files: &[StoredFile]) -> String {
    let mut table = header(HEADER);
    for file in files {
        let size = match file.size_bytes {
            Some(bytes) => bytes.to_string(),
            None => String::new(),
        };
        table.push_str(&row(&[
            file.message_id.to_string(),
            file.mailbox_id.to_string(),
            file.sender.clone(),
            optional(&file.subject),
            file.received_at.clone(),
            file.index.to_string(),
            optional(&file.filename),
            file.content_type.clone(),
            size,
        ]));
    }
    table
}
