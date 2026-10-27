//! What the receiving provider concluded about a message's sender (RFC 8601
//! `Authentication-Results`): each method it checked, such as `spf`, `dkim`
//! or `dmarc`, and its result. Skrzynka checks nothing itself; it records the
//! verdicts as the provider wrote them, so a reader can see whether mail
//! claiming an address was proven to come from it, and a sender can see
//! whether its own mail passes at a mailbox it controls.

use mailparse::{MailHeader, MailHeaderMap};
use serde::{Deserialize, Serialize};

const HEADER: &str = "Authentication-Results";

/// One method's verdict from one `Authentication-Results` header: who wrote
/// it (`authserv_id`), the method, its result, and the rest of the clause
/// (reason and properties) as written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthenticationVerdict {
    pub authserv_id: String,
    pub method: String,
    pub result: String,
    pub detail: Option<String>,
}

/// Every verdict of every `Authentication-Results` header, in header order.
/// A header with no method (`authserv-id; none`) contributes none; a clause
/// that is not `method=result` is kept with the whole clause as `detail` and
/// an empty method and result, so nothing the provider wrote is dropped.
pub(in crate::mail) fn authentication_verdicts(
    headers: &[MailHeader<'_>],
) -> Vec<AuthenticationVerdict> {
    let mut verdicts = Vec::new();
    for value in headers.get_all_values(HEADER) {
        let unfolded = value.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut clauses = unfolded.split(';').map(str::trim);
        let Some(authserv_id) = clauses.next() else {
            continue;
        };
        for clause in clauses.filter(|clause| !clause.is_empty() && *clause != "none") {
            let (head, rest) = match clause.split_once(char::is_whitespace) {
                Some((head, rest)) => (head, Some(rest.trim().to_string())),
                None => (clause, None),
            };
            verdicts.push(match head.split_once('=') {
                Some((method, result)) => AuthenticationVerdict {
                    authserv_id: authserv_id.to_string(),
                    method: method.trim().to_ascii_lowercase(),
                    result: result.trim().to_ascii_lowercase(),
                    detail: rest,
                },
                None => AuthenticationVerdict {
                    authserv_id: authserv_id.to_string(),
                    method: String::new(),
                    result: String::new(),
                    detail: Some(clause.to_string()),
                },
            });
        }
    }
    verdicts
}
