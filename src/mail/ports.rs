//! The ports mail protocols are registered on, read from the services database
//! the operating system ships rather than written here as numbers.

use std::path::Path;

/// The services database the operating system ships: the IANA port registry
/// as `services(5)` lays it out.
const SERVICES_DATABASE: &str = "/etc/services";

/// The TCP port the services database assigns to `service` (a name or an
/// alias, as `imaps`). The refusal names the database and why it gave no port;
/// the caller says what the port was needed for.
pub fn registered_tcp_port(service: &str) -> Result<u16, String> {
    let path = Path::new(SERVICES_DATABASE);
    let text = std::fs::read_to_string(path).map_err(|cause| {
        format!(
            "{} could not be read for the {service} port: {cause}",
            path.display()
        )
    })?;
    let entry = text
        .lines()
        .filter_map(|line| line.split('#').next())
        .find_map(|line| {
            let mut words = line.split_whitespace();
            let name = words.next()?;
            let port = words.next()?.strip_suffix("/tcp")?;
            (name == service || words.any(|alias| alias == service)).then_some(port)
        })
        .ok_or_else(|| format!("{} assigns no TCP port to {service}", path.display()))?;
    entry.parse::<u16>().map_err(|cause| {
        format!(
            "{} assigns {service} the port '{entry}', which is no port: {cause}",
            path.display()
        )
    })
}
