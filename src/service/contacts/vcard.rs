//! Contacts as vCards (RFC 6350), so an address book can take them in.

use super::Contact;

/// Every contact as one vCard, in the order given: `FN` the first name the
/// address was written with, or the address when it was written with none,
/// and `EMAIL` the address. Lines end in CRLF, as RFC 6350 section 3.2 asks.
pub fn vcards(contacts: &[Contact]) -> String {
    let mut cards = String::new();
    for contact in contacts {
        let name = match contact.names.first() {
            Some(name) => name.as_str(),
            None => contact.address.as_str(),
        };
        cards.push_str("BEGIN:VCARD\r\nVERSION:4.0\r\n");
        cards.push_str(&format!("FN:{}\r\n", escaped(name)));
        cards.push_str(&format!("EMAIL:{}\r\n", escaped(&contact.address)));
        cards.push_str("END:VCARD\r\n");
    }
    cards
}

/// A text value with the characters RFC 6350 section 3.4 escapes escaped.
fn escaped(value: &str) -> String {
    let mut written = String::new();
    for character in value.chars() {
        match character {
            '\\' => written.push_str("\\\\"),
            ',' => written.push_str("\\,"),
            ';' => written.push_str("\\;"),
            '\n' => written.push_str("\\n"),
            '\r' => {}
            other => written.push(other),
        }
    }
    written
}
