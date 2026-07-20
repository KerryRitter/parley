//! Loop detection for `par converse` — a cheap, pure heuristic for "have these
//! two agents stopped making progress and started repeating themselves?".
//!
//! Each `ask` is stateless, so `par converse` observes a sequence of full
//! replies (one per turn). When the last few collapse onto the same answer the
//! conversation has stalled, and the loop stops rather than burning the turn
//! budget.

/// FNV-1a 64-bit hash — a tiny, dependency-free, deterministic digest used for
/// reply fingerprints. Not cryptographic; we only need collision resistance
/// good enough to tell "same text" from "different text".
pub(crate) fn fnv1a_64(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in s.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// A short hex fingerprint of arbitrary text.
pub(crate) fn fingerprint(s: &str) -> String {
    format!("{:016x}", fnv1a_64(s.trim()))
}

/// The most times any single signature repeats in a window.
fn max_repeat(signatures: &[String]) -> usize {
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut max = 0;
    for s in signatures {
        let c = counts.entry(s.as_str()).or_insert(0);
        *c += 1;
        max = max.max(*c);
    }
    max
}

/// Across a sequence of full replies (e.g. `par converse` turns), are the last
/// `window` replies collapsing onto the same answer? Used to stop a two-agent
/// loop that has stopped progressing.
pub(crate) fn replies_looping(replies: &[String], window: usize) -> bool {
    if replies.len() < window || window < 2 {
        return false;
    }
    let sigs: Vec<String> = tail(replies, window)
        .iter()
        .map(|r| fingerprint(r))
        .collect();
    // Every reply in the window identical to at least one other => stalled.
    max_repeat(&sigs) >= window
}

fn tail<T>(items: &[T], n: usize) -> &[T] {
    let start = items.len().saturating_sub(n);
    &items[start..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv_is_deterministic_and_distinguishing() {
        assert_eq!(fnv1a_64("abc"), fnv1a_64("abc"));
        assert_ne!(fnv1a_64("abc"), fnv1a_64("abd"));
        assert_eq!(fingerprint(" hi "), fingerprint("hi"));
    }

    #[test]
    fn replies_looping_detects_collapsed_dialogue() {
        let looped: Vec<String> = vec![
            "let's agree".to_string(),
            "I agree".to_string(),
            "I agree".to_string(),
            "I agree".to_string(),
        ];
        assert!(replies_looping(&looped, 3));
        let progressing: Vec<String> = vec![
            "point one".to_string(),
            "point two".to_string(),
            "point three".to_string(),
        ];
        assert!(!replies_looping(&progressing, 3));
    }
}
