//! Límites de frecuencia y detección de duplicados. Lógica pura: el tiempo se
//! inyecta para poder probarla sin esperas.

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::time::{Duration, Instant};

/// Una alerta cada `cooldown` por remitente.
#[derive(Debug)]
pub struct RateLimiter<K> {
    last: HashMap<K, Instant>,
}

impl<K: Eq + Hash + Clone> RateLimiter<K> {
    pub fn new() -> Self {
        Self { last: HashMap::new() }
    }

    /// `Ok` si `key` puede disparar ahora. NO registra: llamar a `record`
    /// solo cuando la alerta se acepta, para que un intento rechazado por
    /// otra causa no consuma el turno.
    pub fn check(&self, key: &K, now: Instant, cooldown: Duration) -> Result<(), Duration> {
        match self.last.get(key) {
            Some(prev) if now.saturating_duration_since(*prev) < cooldown => {
                Err(cooldown - now.saturating_duration_since(*prev))
            }
            _ => Ok(()),
        }
    }

    pub fn record(&mut self, key: K, now: Instant) {
        self.last.insert(key, now);
    }
}

/// Token bucket para limitar mensajes por segundo en una conexión.
#[derive(Debug)]
pub struct TokenBucket {
    capacity: f64,
    tokens: f64,
    refill_per_sec: f64,
    last: Instant,
}

impl TokenBucket {
    pub fn new(capacity: u32, refill_per_sec: f64, now: Instant) -> Self {
        Self { capacity: f64::from(capacity), tokens: f64::from(capacity), refill_per_sec, last: now }
    }

    pub fn try_take(&mut self, now: Instant) -> bool {
        let elapsed = now.saturating_duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + elapsed * self.refill_per_sec).min(self.capacity);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Memoria acotada de ids ya procesados, con la respuesta original para poder
/// repetirla (idempotencia) sin volver a ejecutar la acción.
#[derive(Debug)]
pub struct DedupCache<K, V> {
    entries: HashMap<K, (Instant, Option<V>)>,
    order: VecDeque<K>,
    capacity: usize,
    ttl: Duration,
}

impl<K: Eq + Hash + Clone, V: Clone> DedupCache<K, V> {
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        Self { entries: HashMap::new(), order: VecDeque::new(), capacity, ttl }
    }

    fn evict(&mut self, now: Instant) {
        while let Some(front) = self.order.front() {
            let expired = self.entries.get(front).is_none_or(|(t, _)| now.saturating_duration_since(*t) > self.ttl);
            if expired || self.order.len() > self.capacity {
                if let Some(k) = self.order.pop_front() {
                    self.entries.remove(&k);
                }
            } else {
                break;
            }
        }
    }

    /// Devuelve `Some(respuesta_previa)` si `key` ya se vio; si no, lo registra y devuelve `None`.
    pub fn check_or_insert(&mut self, key: K, now: Instant) -> Option<Option<V>> {
        self.evict(now);
        if let Some((_, resp)) = self.entries.get(&key) {
            return Some(resp.clone());
        }
        self.entries.insert(key.clone(), (now, None));
        self.order.push_back(key);
        None
    }

    pub fn set_response(&mut self, key: &K, response: V) {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.1 = Some(response);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limiter_enforces_cooldown_per_sender() {
        let t0 = Instant::now();
        let cd = Duration::from_secs(15);
        let mut rl = RateLimiter::new();
        assert!(rl.check(&"a", t0, cd).is_ok());
        rl.record("a", t0);
        let wait = rl.check(&"a", t0 + Duration::from_secs(5), cd).unwrap_err();
        assert_eq!(wait, Duration::from_secs(10));
        assert!(rl.check(&"b", t0 + Duration::from_secs(5), cd).is_ok(), "otro remitente no se ve afectado");
        assert!(rl.check(&"a", t0 + Duration::from_secs(15), cd).is_ok());
    }

    #[test]
    fn unrecorded_attempts_do_not_consume_the_slot() {
        let t0 = Instant::now();
        let rl = RateLimiter::new();
        assert!(rl.check(&"a", t0, Duration::from_secs(15)).is_ok());
        assert!(rl.check(&"a", t0, Duration::from_secs(15)).is_ok());
    }

    #[test]
    fn token_bucket_limits_bursts_and_refills() {
        let t0 = Instant::now();
        let mut b = TokenBucket::new(3, 1.0, t0);
        assert!(b.try_take(t0) && b.try_take(t0) && b.try_take(t0));
        assert!(!b.try_take(t0));
        assert!(b.try_take(t0 + Duration::from_millis(1100)));
        assert!(!b.try_take(t0 + Duration::from_millis(1100)));
    }

    #[test]
    fn dedup_detects_replays_and_returns_original_response() {
        let t0 = Instant::now();
        let mut d: DedupCache<u32, &str> = DedupCache::new(8, Duration::from_secs(60));
        assert_eq!(d.check_or_insert(1, t0), None);
        assert_eq!(d.check_or_insert(1, t0), Some(None), "visto pero aún sin respuesta");
        d.set_response(&1, "ok");
        assert_eq!(d.check_or_insert(1, t0), Some(Some("ok")));
        assert_eq!(d.check_or_insert(2, t0), None);
    }

    #[test]
    fn dedup_is_bounded_and_expires() {
        let t0 = Instant::now();
        let mut d: DedupCache<u32, ()> = DedupCache::new(3, Duration::from_secs(60));
        for i in 0..10 {
            d.check_or_insert(i, t0);
        }
        assert!(d.entries.len() <= 4, "capacidad acotada: {}", d.entries.len());
        assert_eq!(d.check_or_insert(0, t0), None, "los más viejos se descartan");
        assert_eq!(d.check_or_insert(9, t0), Some(None));
        assert_eq!(d.check_or_insert(9, t0 + Duration::from_secs(61)), None, "expira por TTL");
    }
}
