package io.github.leoheco.serlog;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.leoheco.serlog.alarm.Tiers;
import io.github.leoheco.serlog.process.Match;
import io.github.leoheco.serlog.sink.FileSink;
import io.github.leoheco.serlog.sink.MemorySink;
import java.io.IOException;
import java.nio.file.Path;
import java.time.Duration;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.concurrent.atomic.AtomicInteger;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.extension.ExtendWith;
import org.junit.jupiter.api.io.TempDir;

/// Slots at run time: names, claims, start-up checks, queues, health, failure policies, alarms and loops.
@ExtendWith(Clean.class)
class SlotsTest {
    static final class Flaky extends SinkBase<Flaky> {
        volatile boolean fail;
        final AtomicInteger seen = new AtomicInteger();

        @Override
        public String kind() {
            return "flaky";
        }

        @Override
        public void emit(Entry entry) throws IOException {
            seen.incrementAndGet();
            if (fail) throw new IOException("boom");
        }
    }

    private static Status status(String name) {
        return Serlog.status().stream()
                .filter(s -> s.name().equals(name))
                .findFirst()
                .orElseThrow();
    }

    private static String only(String target) {
        return target + "=trace";
    }

    @Test
    void aFailureIsToldOnceAndItsRecoveryAfter() {
        var alarms = new MemorySink(10);
        Serlog.add(Slot.of(alarms).name("alarms1").alarmOnly(true));
        var flaky = new Flaky()
                .name("flaky1")
                .filter(only("t1"))
                .probe(Duration.ZERO)
                .onFail(new Tiers(List.of(List.of("alarms1"))));
        flaky.fail = true;
        Serlog.add(flaky);
        for (var i = 0; i < 3; i++) Serlog.get("t1").info("write");
        Serlog.flush();
        assertEquals(3, flaky.seen.get());
        assertEquals(Status.Health.DOWN, status("flaky1").health());
        var told = alarms.records();
        assertEquals(1, told.size());
        assertEquals(Level.ERROR, told.getFirst().level());
        assertEquals("sink `flaky1` failed: boom", told.getFirst().msg());
        assertEquals("opened", told.getFirst().field("phase"));
        flaky.fail = false;
        Serlog.get("t1").info("write");
        Serlog.flush();
        assertEquals(Status.Health.UP, status("flaky1").health());
        assertEquals(
                List.of("opened", "resolved"),
                alarms.records().stream().map(r -> r.field("phase")).toList());
    }

    @Test
    void aDownSinkIsSkippedUntilItsProbe() {
        var flaky = new Flaky();
        flaky.fail = true;
        Serlog.add(Slot.of(flaky)
                .name("flaky2")
                .filter(only("t2"))
                .probe(Duration.ofHours(1))
                .onFail("ignore"));
        for (var i = 0; i < 3; i++) Serlog.get("t2").info("write");
        assertEquals(1, flaky.seen.get());
        var s = status("flaky2");
        assertEquals(List.of(Status.Health.DOWN, 2L, "boom"), List.of(s.health(), s.skipped(), s.error()));
    }

    @Test
    void thePanicPolicyThrowsInTheCallerEveryTime() {
        var flaky = new Flaky().name("flaky3").filter(only("t3")).onFail("panic");
        flaky.fail = true;
        Serlog.add(flaky);
        for (var i = 0; i < 2; i++) {
            var e = assertThrows(SerlogException.class, () -> Serlog.get("t3").error("write"));
            assertTrue(e.getMessage().contains("flaky3"));
        }
        assertEquals(2, flaky.seen.get());
    }

    @Test
    void namesClaimsAndChecksAreEnforced(@TempDir Path dir) {
        var a = Serlog.add(new MemorySink(1).filter("off"));
        var b = Serlog.add(new MemorySink(1).filter("off"));
        assertTrue(a.startsWith("memory-") && b.startsWith("memory-"));
        assertNotEquals(a, b);
        assertThrows(SerlogException.NameTaken.class, () -> Serlog.add(new MemorySink().name("main")));
        assertThrows(SerlogException.NameTaken.class, () -> Serlog.add(new MemorySink().name("*")));
        var path = dir.resolve("one.log");
        Serlog.add(new FileSink(path).name("one").filter("off"));
        assertThrows(SerlogException.Claimed.class, () -> Serlog.add(new FileSink(path).name("two")));
        assertThrows(SerlogException.NotReady.class, () -> Serlog.add(new FileSink(dir).name("three")));
        assertTrue(Serlog.remove("one"));
        assertTrue(!Serlog.remove("one"));
    }

    @Test
    void aQueuedSlotWritesInTheBackground() {
        var mem = new MemorySink(100);
        Serlog.add(Slot.of(mem).name("queued").filter(only("t5")).queue(100).wait(Duration.ofMillis(20)));
        for (var n = 0; n < 5; n++) Serlog.get("t5").atInfo().field("n", n).log("queued");
        Serlog.flush();
        assertEquals(5, mem.size());
    }

    @Test
    void slotsLayOutAndProcessOnTheirOwn() {
        var js = new MemorySink(10);
        var text = new MemorySink(10);
        Serlog.add(js.name("json").filter(only("t6")).format("json"));
        Serlog.add(text.name("text").filter(only("t6")).pattern("{level}:{msg}").process(Match.tags("keep")));
        var log = Serlog.get("t6");
        log.atInfo().tag("keep").log("kept");
        log.info("dropped by text");
        assertEquals(2, js.size());
        assertTrue(js.lines().getFirst().startsWith("{\"time\":"));
        assertEquals(List.of("INFO:kept"), text.lines());
    }

    @Test
    void slotKeysComeFromConfigEntriesToo() {
        var mem = new MemorySink(10);
        Serlog.add(Slot.of(mem)
                .with(Map.of(
                        "name",
                        "entry",
                        "filter",
                        only("t10"),
                        "pattern",
                        "{msg}",
                        "process",
                        List.of(Map.of("fields", Map.of("a", 1))))));
        Serlog.get("t10").info("x");
        assertEquals(List.of("x"), mem.lines());
        assertEquals(1, mem.records().getFirst().field("a"));
    }

    @Test
    void recordsMadeInsideASinkAreSuppressed() {
        var seen = new AtomicInteger();
        Serlog.add(Slot.of(e -> {
                    seen.incrementAndGet();
                    Serlog.get("t7").info("from inside the sink");
                })
                .name("loud")
                .filter(only("t7")));
        Serlog.get("t7").info("outside");
        assertEquals(1, seen.get());
        assertEquals(1, status("loud").suppressed());
    }

    @Test
    void recordsMadeInAQueuedSinkAreSuppressed() {
        var seen = new AtomicInteger();
        Serlog.add(Slot.of(e -> {
                    seen.incrementAndGet();
                    Serlog.get("t9").info("from inside, later");
                })
                .name("slow")
                .filter(only("t9"))
                .queue(10)
                .wait(Duration.ofMillis(1)));
        Serlog.get("t9").info("outside");
        Serlog.flush();
        assertEquals(1, seen.get());
        assertEquals(1, status("slow").suppressed());
    }

    @Test
    void removingASlotWritesItsQueueAndClosesItsSink(@TempDir Path dir) throws IOException {
        var mem = new MemorySink(10);
        Serlog.add(Slot.of(mem).name("leaving").filter(only("t11")).queue(10).wait(Duration.ofHours(1)));
        for (var i = 0; i < 3; i++) Serlog.get("t11").info("queued");
        var path = dir.resolve("gone.log");
        Serlog.add(new FileSink(path).name("gone").filter("off"));
        assertTrue(Serlog.remove("leaving") && Serlog.remove("gone"));
        assertEquals(3, mem.size());
        java.nio.file.Files.delete(path);
    }

    @Test
    void lambdasAreSinks() {
        var got = new ArrayList<String>();
        Serlog.add(Slot.of(e -> got.add(e.line())).name("fn").filter(only("t8")).pattern("{msg}"));
        Serlog.get("t8").warn("hi");
        assertEquals(List.of("hi"), got);
    }
}
