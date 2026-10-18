//! A slot's queue and the task draining it, both made when the first record arrives.

use std::{
    sync::{
        Arc, OnceLock,
        atomic::Ordering,
        mpsc::{self, RecvTimeoutError, SyncSender, TrySendError},
    },
    thread,
    time::{Duration, Instant},
};

use super::{Core, Full, Queue};
use crate::{RecordBuf, guard};

enum Msg {
    Item(Box<RecordBuf>),
    Flush(SyncSender<()>),
}

pub(crate) struct Lane {
    queue: Queue,
    core: Arc<Core>,
    tx: OnceLock<Option<SyncSender<Msg>>>,
}

impl Lane {
    pub(crate) const fn new(queue: Queue, core: Arc<Core>) -> Self {
        Self {
            queue,
            core,
            tx: OnceLock::new(),
        }
    }

    pub(crate) fn push(&self, rec: RecordBuf) {
        let Some(tx) = self.tx.get_or_init(|| start(self.queue, self.core.clone())) else {
            self.core.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        };
        let msg = Msg::Item(Box::new(rec));
        let sent = if self.queue.full == Full::Block && self.core.up() {
            tx.send(msg).is_ok()
        } else {
            tx.try_send(msg).is_ok()
        };
        if !sent {
            self.core.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Waits until what is queued has been written, unless the sink is down or `deadline` passes.
    pub(crate) fn flush(&self, deadline: Instant) {
        let Some(Some(tx)) = self.tx.get() else {
            return;
        };
        let (ack, done) = mpsc::sync_channel(1);
        let mut msg = Msg::Flush(ack);
        loop {
            if !self.core.up() || Instant::now() >= deadline {
                return;
            }
            match tx.try_send(msg) {
                Ok(()) => break,
                Err(TrySendError::Full(m)) => {
                    msg = m;
                    thread::sleep(Duration::from_millis(5));
                }
                Err(TrySendError::Disconnected(_)) => return,
            }
        }
        let _ = done.recv_timeout(deadline.saturating_duration_since(Instant::now()));
    }
}

fn start(queue: Queue, core: Arc<Core>) -> Option<SyncSender<Msg>> {
    let (tx, rx) = mpsc::sync_channel(queue.size.max(1));
    let batch = queue.batch.max(1);
    thread::Builder::new()
        .name(format!("serlog-{}", core.name))
        .spawn(move || {
            guard::adopt(Some(core.clone()));
            let mut buf = Vec::with_capacity(batch);
            let mut due = Instant::now();
            loop {
                let msg = if buf.is_empty() {
                    rx.recv().map_err(|_| RecvTimeoutError::Disconnected)
                } else {
                    rx.recv_timeout(due.saturating_duration_since(Instant::now()))
                };
                match msg {
                    Ok(Msg::Item(rec)) => {
                        if buf.is_empty() {
                            due = Instant::now() + queue.wait;
                        }
                        buf.push(*rec);
                        if buf.len() >= batch {
                            core.drain(&mut buf, true);
                        }
                    }
                    Ok(Msg::Flush(ack)) => {
                        core.drain(&mut buf, true);
                        if let Err(e) = core.sink.flush() {
                            core.fault(&e, core.tell());
                        }
                        let _ = ack.send(());
                    }
                    Err(RecvTimeoutError::Timeout) => core.drain(&mut buf, true),
                    Err(RecvTimeoutError::Disconnected) => {
                        core.drain(&mut buf, false);
                        let _ = core.sink.flush();
                        break;
                    }
                }
            }
        })
        .ok()
        .map(|_| tx)
}
