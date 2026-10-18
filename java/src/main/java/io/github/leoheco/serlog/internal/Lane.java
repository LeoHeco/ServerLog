package io.github.leoheco.serlog.internal;

import io.github.leoheco.serlog.LogRecord;
import io.github.leoheco.serlog.Queued;
import java.time.Duration;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.locks.Condition;
import java.util.concurrent.locks.ReentrantLock;

/// A slot's queue and the virtual thread draining it, both made when the first record arrives.
final class Lane {
    final Queued shape;
    private final Core core;
    private final ArrayDeque<LogRecord> buf = new ArrayDeque<>();
    private final ReentrantLock lock = new ReentrantLock();
    private final Condition more = lock.newCondition();
    private final Condition space = lock.newCondition();
    private final Condition idle = lock.newCondition();
    private Thread worker;
    private boolean busy;
    private boolean urgent;
    private boolean stopped;

    Lane(Queued shape, Core core) {
        this.shape = shape;
        this.core = core;
    }

    /// Queues `rec`; a full queue drops it, or with `full: block` waits while the sink is up.
    void push(LogRecord rec) {
        lock.lock();
        try {
            while (buf.size() >= shape.size()) {
                if (shape.full() != Queued.Full.BLOCK || !core.up()) {
                    core.dropped.incrementAndGet();
                    return;
                }
                space.await(50, TimeUnit.MILLISECONDS);
            }
            buf.add(rec);
            if (buf.size() >= shape.batch()) more.signal();
            if (worker == null) {
                busy = true;
                worker = Thread.ofVirtual().name("serlog-" + core.name).start(() -> Guard.run(core, this::run));
            }
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            core.dropped.incrementAndGet();
        } finally {
            lock.unlock();
        }
    }

    private void run() {
        var batch = new ArrayList<LogRecord>(Math.min(shape.batch(), 4096));
        for (; ; ) {
            lock.lock();
            try {
                while (buf.isEmpty()) {
                    if (stopped) return;
                    busy = false;
                    urgent = false;
                    idle.signalAll();
                    more.awaitUninterruptibly();
                }
                busy = true;
                var left = shape.delay().toNanos();
                while (buf.size() < shape.batch() && !urgent && left > 0) left = more.awaitNanos(left);
                for (var n = Math.min(buf.size(), shape.batch()); n > 0; n--) batch.add(buf.poll());
                space.signalAll();
            } catch (InterruptedException e) {
                return;
            } finally {
                lock.unlock();
            }
            core.drain(batch);
            batch.clear();
        }
    }

    /// Lets the worker end once the queue is empty.
    void stop() {
        lock.lock();
        try {
            stopped = true;
            more.signalAll();
        } finally {
            lock.unlock();
        }
    }

    /// Waits until what is queued has been written, unless the sink is down or the time is up.
    void flush(Duration within) {
        var left = within.toNanos();
        lock.lock();
        try {
            if (worker == null) return;
            urgent = true;
            more.signal();
            while ((busy || !buf.isEmpty()) && core.up() && left > 0) left = idle.awaitNanos(left);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        } finally {
            lock.unlock();
        }
    }
}
