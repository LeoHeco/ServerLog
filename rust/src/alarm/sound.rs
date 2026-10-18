//! Walking an alarm's tiers: the same rules in every language, checked by `spec/cases/alarm.json`.

use std::{collections::HashSet, thread, time::SystemTime};

use super::{Alarm, Failure, Incident};
use crate::RecordBuf;

/// What the alarm walk needs from the sinks around it.
pub trait Desk: Sync {
    /// Names of the sinks on the log route, which `*` stands for.
    fn logs(&self) -> Vec<String>;

    /// Whether the sink is down and not yet due for a probe, so it is left out.
    fn resting(&self, name: &str) -> bool;

    /// Writes `rec` to the named sink straight away, bypassing its queue.
    ///
    /// # Errors
    /// What the sink said, or that no sink has that name.
    fn deliver(&self, name: &str, rec: &RecordBuf) -> Result<(), String>;
}

/// How an alarm walk ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// A sink took the incident.
    Heard,
    /// No tier took it.
    Unheard,
}

/// Delivers `incident` through `alarm`'s tiers, adding every failed delivery to it.
pub fn sound(alarm: &dyn Alarm, incident: &mut Incident, desk: &impl Desk) -> Outcome {
    let mut out: HashSet<String> = HashSet::from([incident.sink.clone()]);
    for tier in alarm.route(incident) {
        let mut names: Vec<String> = Vec::new();
        for n in tier {
            for m in if n == "*" { desk.logs() } else { vec![n] } {
                if !out.contains(&m) && !names.contains(&m) && !desk.resting(&m) {
                    names.push(m);
                }
            }
        }
        if names.is_empty() {
            continue;
        }
        let rec = alarm.describe(incident);
        let results: Vec<(String, Result<(), String>)> = match names.as_slice() {
            [one] => vec![(one.clone(), desk.deliver(one, &rec))],
            _ => thread::scope(|s| {
                let jobs: Vec<_> = names
                    .iter()
                    .map(|n| (n, s.spawn(|| desk.deliver(n, &rec))))
                    .collect();
                jobs.into_iter()
                    .map(|(n, j)| {
                        (
                            n.clone(),
                            j.join().unwrap_or_else(|_| Err("panicked".into())),
                        )
                    })
                    .collect()
            }),
        };
        let mut heard = false;
        for (sink, r) in results {
            match r {
                Ok(()) => heard = true,
                Err(error) => incident.failures.push(Failure {
                    sink: sink.clone(),
                    error,
                    time: SystemTime::now(),
                }),
            }
            out.insert(sink);
        }
        if heard {
            return Outcome::Heard;
        }
    }
    Outcome::Unheard
}
