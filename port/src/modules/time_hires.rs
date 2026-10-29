//! Time::HiRes, the part ExifTool uses: gettimeofday and tv_interval, for
//! the ProcessingTime tag (`-api ProcessingTime=1`) and Geolocation's
//! timings.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::interp::eval::Want;
use crate::interp::Interp;
use crate::perl::Value;
use crate::pl::arg;

/// The time of day as (seconds, microseconds), as gettimeofday(2).
fn now() -> (i64, i64) {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => (d.as_secs() as i64, d.subsec_micros() as i64),
        Err(e) => {
            // (a clock before 1970)
            let d = e.duration();
            let (s, u) = (-(d.as_secs() as i64), -(d.subsec_micros() as i64));
            if u < 0 {
                (s - 1, u + 1_000_000)
            } else {
                (s, u)
            }
        }
    }
}

/// tv_interval's arithmetic: `($b->[0] - $a->[0]) + (($b->[1] - $a->[1]) / 1_000_000)`.
pub fn interval(a: (f64, f64), b: (f64, f64)) -> f64 {
    (b.0 - a.0) + ((b.1 - a.1) / 1_000_000.0)
}

pub fn register(it: &mut Interp) {
    it.set_scalar("Time::HiRes::VERSION", Value::str("1.9775"));
    // gettimeofday(): (seconds, microseconds) in list context, the
    // floating-point seconds in scalar context
    it.define_native("Time::HiRes::gettimeofday", |_, _, want| {
        let (s, u) = now();
        Ok(if want == Want::List {
            vec![Value::Int(s), Value::Int(u)]
        } else {
            vec![Value::Num(s as f64 + u as f64 / 1e6)]
        })
    });
    // tv_interval(\@start, [\@end]): the seconds between two
    // gettimeofday lists (the end is now when omitted)
    it.define_native("Time::HiRes::tv_interval", |_, a, _| {
        let pair = |v: &Value| (v.at(0).to_f64(), v.at(1).to_f64());
        let start = pair(&arg(&a, 0));
        let end = match a.get(1) {
            Some(v) if !v.is_undef() => pair(v),
            _ => {
                let (s, u) = now();
                (s as f64, u as f64)
            }
        };
        Ok(vec![Value::Num(interval(start, end))])
    });
    // time(): the floating-point seconds
    it.define_native("Time::HiRes::time", |_, _, _| {
        let (s, u) = now();
        Ok(vec![Value::Num(s as f64 + u as f64 / 1e6)])
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tv_interval_arithmetic() {
        assert_eq!(interval((10.0, 500_000.0), (12.0, 250_000.0)), 1.75);
        assert_eq!(interval((10.0, 0.0), (10.0, 1.0)), 0.000001);
    }

    #[test]
    fn gettimeofday_microseconds() {
        let (s, u) = now();
        assert!(s > 1_600_000_000);
        assert!((0..1_000_000).contains(&u));
    }
}
