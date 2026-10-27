//! CSV tables (RFC 4180) of what Skrzynka lists, so a spreadsheet, a CRM or
//! a report can take the listing in: the stored mail, the outbound log, the
//! addresses that bounced and the contacts, each under a header naming the
//! fields its listing prints; and the reading of a table a spreadsheet
//! wrote, which a mail merge takes.

mod bounced;
mod drafts;
mod messages;
mod outbound;
pub use bounced::bounced_table;
pub use drafts::draft_table;
pub use messages::message_table;
pub use outbound::{outbound_table, reply_table};

/// One CSV line of fields: each holding a comma, a quote or a line break
/// quoted with its quotes doubled, the line ended in CRLF. Every table
/// Skrzynka writes uses it.
pub(in crate::service) fn row(fields: &[String]) -> String {
    let quoted: Vec<String> = fields
        .iter()
        .map(|field| match field.contains([',', '"', '\r', '\n']) {
            true => format!("\"{}\"", field.replace('"', "\"\"")),
            false => field.clone(),
        })
        .collect();
    format!("{}\r\n", quoted.join(","))
}

/// The rows of a CSV table as a spreadsheet writes it: fields split at
/// commas outside quotes, a quoted field's doubled quotes read as one and its
/// commas and line breaks kept, lines ended by CRLF or LF, and lines holding
/// nothing but blanks left out. A quote never closed is refused with why.
pub(in crate::service) fn read_rows(text: &str) -> Result<Vec<Vec<String>>, String> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(char) = chars.next() {
        match (quoted, char) {
            (true, '"') if chars.peek() == Some(&'"') => {
                chars.next();
                field.push('"');
            }
            (true, '"') => quoted = false,
            (true, other) => field.push(other),
            (false, '"') if field.is_empty() => quoted = true,
            (false, ',') => row.push(std::mem::take(&mut field)),
            (false, '\r') if chars.peek() == Some(&'\n') => {}
            (false, '\n') => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            (false, other) => field.push(other),
        }
    }
    if quoted {
        return Err("a quoted field is never closed".to_string());
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows.retain(|kept: &Vec<String>| kept.iter().any(|value| !value.trim().is_empty()));
    Ok(rows)
}

/// The header's names as the row `row` writes first.
pub(in crate::service) fn header(names: &[&str]) -> String {
    let fields: Vec<String> = names.iter().map(|name| name.to_string()).collect();
    row(&fields)
}

/// A field a row may not hold, written empty when it is absent.
pub(in crate::service) fn optional(value: &Option<String>) -> String {
    match value {
        Some(written) => written.clone(),
        None => String::new(),
    }
}
