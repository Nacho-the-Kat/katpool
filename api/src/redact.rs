//! Address redaction for logs and traces.
//!
//! Wallet addresses are pseudonymous but linkable. The threat model
//! (`docs/threat-model.md`) treats them as semi-sensitive and forbids
//! emitting full addresses into logs. Responses still return the full
//! address the caller already supplied — redaction applies only to the
//! telemetry side (span fields, error logs), never to the response body.

/// Redact an address to a stable, low-information telemetry tag.
///
/// Keeps the network prefix (up to and including the first `:`) plus the last
/// four characters, e.g. `kaspa:…s9jx`. Short or prefixless inputs degrade
/// gracefully.
#[must_use]
pub fn address(addr: &str) -> String {
    let tail: String = addr
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    match addr.split_once(':') {
        Some((prefix, _)) => format!("{prefix}:…{tail}"),
        None => format!("…{tail}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_prefix_and_last_four() {
        let r = address("kaspa:qz4j8mu269z8llgcczmfukm9fan2fq822kzxu4cfukd5fqrhxpsv2zhs9jxnp");
        assert_eq!(r, "kaspa:…jxnp");
    }

    #[test]
    fn exact_tail_is_four_chars() {
        let r = address("kaspa:abcdef1234");
        assert_eq!(r, "kaspa:…1234");
    }

    #[test]
    fn prefixless_input_degrades() {
        assert_eq!(address("abcdef"), "…cdef");
    }

    #[test]
    fn never_contains_full_body() {
        let full = "kaspa:qz4j8mu269z8llgcczmfukm9fan2fq822kzxu4cfukd5fqrhxpsv2zhs9jxnp";
        let r = address(full);
        assert!(!r.contains("qz4j8mu269"));
    }
}
