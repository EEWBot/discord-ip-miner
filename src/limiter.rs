use std::time::{Duration, Instant};

use papaya::{HashMap, HashSet};

use crate::request::Request;

#[derive(Debug)]
pub enum Status {
    Pass,
    Ratelimited(Duration),
    Known404,
}

#[derive(Debug, Default)]
pub struct Limiter {
    notfound_set: HashSet<url::Url>,
    ratelimits: HashMap<url::Url, Instant>,
}

impl Limiter {
    pub fn current(&self, request: &Request) -> Status {
        if self.notfound_set.pin().contains(&request.target) {
            return Status::Known404;
        }

        if let Some(ratelimit_to) = self.ratelimits.pin().get(&request.target) {
            if let Some(duration) = ratelimit_to.checked_duration_since(Instant::now()) {
                return Status::Ratelimited(duration);
            }
        }

        Status::Pass
    }

    pub fn tell_notfound(&self, target: &url::Url) {
        self.notfound_set.pin().insert(target.to_owned());
    }

    pub fn tell_ratelimit(&self, target: &url::Url, retry_after: f32) -> Duration {
        let delta_time = Duration::from_secs_f32(retry_after);
        let limit_to = Instant::now() + delta_time;

        let ratelimit_to = *self.ratelimits.pin().update_or_insert(
            target.to_owned(),
            |current| (*current).max(limit_to),
            limit_to,
        );

        match ratelimit_to.checked_duration_since(Instant::now()) {
            Some(value) => value,
            None => Duration::ZERO,
        }
    }
}
