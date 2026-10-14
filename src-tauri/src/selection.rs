//! Selección aleatoria pura (sin I/O) para poder probarla de forma determinista.

/// Elige un elemento de `candidates`.
///
/// - Si hay al menos dos candidatos, excluye el que tenga id `last`
///   (no repetir inmediatamente la última canción).
/// - Con un único candidato lo devuelve aunque sea el último.
/// - `rng(n)` debe devolver un índice en `0..n`; se corrige con módulo
///   por seguridad.
pub fn pick<'a, T>(
    candidates: &'a [T],
    id_of: impl Fn(&T) -> &str,
    last: Option<&str>,
    rng: impl FnOnce(usize) -> usize,
) -> Option<&'a T> {
    match candidates.len() {
        0 => None,
        1 => candidates.first(),
        _ => {
            let pool: Vec<&T> = candidates
                .iter()
                .filter(|c| last != Some(id_of(c)))
                .collect();
            let pool_len = pool.len();
            // `pool` solo queda vacío si todos comparten id; en ese caso
            // no hay nada que excluir.
            if pool_len == 0 {
                let i = rng(candidates.len()) % candidates.len();
                return candidates.get(i);
            }
            let i = rng(pool_len) % pool_len;
            pool.get(i).copied()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&'static str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn empty_returns_none() {
        let c: Vec<String> = vec![];
        assert!(pick(&c, |s| s.as_str(), None, |_| 0).is_none());
    }

    #[test]
    fn single_candidate_is_returned_even_if_last() {
        let c = ids(&["a"]);
        assert_eq!(pick(&c, |s| s.as_str(), Some("a"), |_| 0).unwrap(), "a");
    }

    #[test]
    fn never_repeats_last_with_two_or_more() {
        let c = ids(&["a", "b", "c"]);
        for r in 0..10 {
            let got = pick(&c, |s| s.as_str(), Some("b"), |n| r % n).unwrap();
            assert_ne!(got, "b");
        }
    }

    #[test]
    fn two_candidates_alternate_deterministically() {
        let c = ids(&["a", "b"]);
        assert_eq!(pick(&c, |s| s.as_str(), Some("a"), |_| 0).unwrap(), "b");
        assert_eq!(pick(&c, |s| s.as_str(), Some("b"), |_| 0).unwrap(), "a");
    }

    #[test]
    fn unknown_last_keeps_all_candidates() {
        let c = ids(&["a", "b", "c"]);
        let mut seen = std::collections::HashSet::new();
        for r in 0..3 {
            seen.insert(pick(&c, |s| s.as_str(), Some("zzz"), |_| r).unwrap().clone());
        }
        assert_eq!(seen.len(), 3);
    }

    #[test]
    fn out_of_range_rng_is_wrapped() {
        let c = ids(&["a", "b"]);
        assert!(pick(&c, |s| s.as_str(), None, |_| 9999).is_some());
    }
}
