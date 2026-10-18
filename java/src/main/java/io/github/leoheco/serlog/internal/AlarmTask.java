package io.github.leoheco.serlog.internal;

import io.github.leoheco.serlog.alarm.Alarm;
import io.github.leoheco.serlog.alarm.Incident;
import java.time.Duration;
import java.util.ArrayDeque;
import java.util.concurrent.atomic.AtomicLong;
import java.util.concurrent.locks.Condition;
import java.util.concurrent.locks.ReentrantLock;

/// The alarm route: one queue and one task, both made when the first incident arrives.
public final class AlarmTask {
    private record Job(Alarm alarm, Incident incident) {}

    private static final int LIMIT = 64;
    private static final ArrayDeque<Job> JOBS = new ArrayDeque<>();
    private static final ReentrantLock LOCK = new ReentrantLock();
    private static final Condition MORE = LOCK.newCondition();
    private static final Condition IDLE = LOCK.newCondition();
    private static final AtomicLong LOST = new AtomicLong();
    private static Thread worker;
    private static boolean busy;
    private static volatile boolean crashing;

    private AlarmTask() {}

    /// Queues `incident` for `alarm`; never blocks, and counts what a full queue drops.
    public static void post(Alarm alarm, Incident incident) {
        LOCK.lock();
        try {
            if (JOBS.size() >= LIMIT) {
                LOST.incrementAndGet();
                return;
            }
            JOBS.add(new Job(Hub.resolve(alarm), incident));
            MORE.signal();
            if (worker == null)
                worker = Thread.ofVirtual().name("serlog-alarm").start(() -> Guard.run(null, AlarmTask::drain));
        } finally {
            LOCK.unlock();
        }
    }

    private static void drain() {
        for (; ; ) {
            Job job;
            LOCK.lock();
            try {
                while (JOBS.isEmpty()) {
                    busy = false;
                    IDLE.signalAll();
                    MORE.awaitUninterruptibly();
                }
                job = JOBS.poll();
                busy = true;
            } finally {
                LOCK.unlock();
            }
            var heard = Walk.sound(job.alarm(), job.incident(), Hub.BOARD);
            if (!heard && job.alarm().fallback() == Alarm.Fallback.PANIC)
                crash("no sink took the alarm for `" + job.incident().sink() + "`: "
                        + job.incident().error());
        }
    }

    /// Waits up to `within` for the incidents queued so far.
    public static void settle(Duration within) {
        var left = within.toNanos();
        LOCK.lock();
        try {
            while ((busy || !JOBS.isEmpty()) && left > 0) left = IDLE.awaitNanos(left);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        } finally {
            LOCK.unlock();
        }
    }

    /// Incidents a full alarm queue dropped.
    public static long dropped() {
        return LOST.get();
    }

    /// Whether a crash is under way, so the exit hook does not wait again.
    public static boolean crashing() {
        return crashing;
    }

    /// Says why on standard error, gives the other sinks a second to flush, and exits with 70.
    public static void crash(String why) {
        crashing = true;
        try {
            System.err.println("serlog: " + why + "; exiting");
            System.err.flush();
        } catch (RuntimeException _) {
        }
        Hub.flushSlots(Duration.ofSeconds(1));
        Runtime.getRuntime().halt(70);
    }
}
