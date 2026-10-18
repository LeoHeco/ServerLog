//! The alarm route: one queue and one task, both made when the first incident arrives.

use std::{
    io::{self, Write},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, SyncSender},
    },
    thread,
    time::Duration,
};

use super::{Alarm, Fallback, Incident, Outcome, sound};
use crate::{guard, hub};

enum Msg {
    Job(Arc<dyn Alarm>, Box<Incident>),
    Settle(SyncSender<()>),
}

static TX: OnceLock<Option<SyncSender<Msg>>> = OnceLock::new();
static LOST: AtomicU64 = AtomicU64::new(0);

/// Queues `incident` for `alarm`; never blocks, and counts what a full queue drops.
pub(crate) fn raise(alarm: Arc<dyn Alarm>, incident: Incident) {
    let sent = TX
        .get_or_init(start)
        .as_ref()
        .is_some_and(|tx| tx.try_send(Msg::Job(alarm, Box::new(incident))).is_ok());
    if !sent {
        LOST.fetch_add(1, Ordering::Relaxed);
    }
}

fn start() -> Option<SyncSender<Msg>> {
    let (tx, rx) = mpsc::sync_channel(64);
    thread::Builder::new()
        .name("serlog-alarm".into())
        .spawn(move || {
            guard::adopt(None);
            for msg in rx {
                match msg {
                    Msg::Job(alarm, mut incident) => {
                        if sound(&*alarm, &mut incident, &hub::Board) == Outcome::Unheard
                            && alarm.fallback() == Fallback::Panic
                        {
                            crash(&format!(
                                "no sink took the alarm for `{}`: {}",
                                incident.sink, incident.error
                            ));
                        }
                    }
                    Msg::Settle(ack) => {
                        let _ = ack.send(());
                    }
                }
            }
        })
        .ok()
        .map(|_| tx)
}

/// Waits up to `timeout` for the incidents queued so far.
pub(crate) fn settle(timeout: Duration) {
    let Some(Some(tx)) = TX.get() else {
        return;
    };
    let (ack, done) = mpsc::sync_channel(1);
    if tx.try_send(Msg::Settle(ack)).is_ok() {
        let _ = done.recv_timeout(timeout);
    }
}

/// Incidents a full alarm queue dropped.
pub fn dropped() -> u64 {
    LOST.load(Ordering::Relaxed)
}

/// Says why on standard error, gives the other sinks a second to flush, and exits with 70.
pub(crate) fn crash(why: &str) -> ! {
    let _ = writeln!(io::stderr(), "serlog: {why}; exiting");
    hub::flush_slots(Duration::from_secs(1));
    std::process::exit(70)
}
