package io.github.leoheco.serlog;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.leoheco.serlog.sink.MemorySink;
import java.util.List;
import java.util.Map;
import java.util.concurrent.atomic.AtomicInteger;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.extension.ExtendWith;

/// Making records: messages, typed fields, tags, the caller, scopes, laziness, errors and SLF4J.
@ExtendWith(Clean.class)
class CallsTest {
    private MemorySink mem;

    @BeforeEach
    void sink() {
        mem = new MemorySink(100);
        Serlog.add(mem.filter("calls=trace,"
                        + (Serlog.meta().app().isEmpty()
                                ? "main"
                                : Serlog.meta().app()) + "=trace,slf=trace")
                .pattern("{file}:{func} {msg}"));
    }

    private LogRecord last(String msg) {
        var r = mem.last(msg);
        assertNotNull(r, "no record \"" + msg + "\"");
        return r;
    }

    @Test
    void messagesFieldsAndTheCaller() {
        var sync = Serlog.get("calls", Map.of("mirror", "debian"));
        sync.atInfo()
                .field("tries", 3)
                .field("ratio", 0.5)
                .field("ok", true)
                .tag("sync")
                .log("synced {} files", 42);
        var r = last("synced 42 files");
        assertEquals(
                List.of(
                        Field.of("mirror", "debian"),
                        Field.of("tries", 3),
                        Field.of("ratio", 0.5),
                        Field.of("ok", true)),
                r.fields());
        assertEquals(List.of("sync"), r.tags());
        assertEquals("messagesFieldsAndTheCaller", r.func());
        assertEquals("CallsTest.java", r.file());
        assertEquals(CallsTest.class.getName(), r.module());
        assertTrue(r.line() > 0);
    }

    @Test
    void placeholdersEscapesAndTrailingErrors() {
        var log = Serlog.get("calls");
        log.info("{} and \\{} and {}", 1, 2);
        log.error("failed {}", "x", new IllegalStateException("bad"));
        assertEquals("1 and {} and 2", last("1 and {} and 2").msg());
        assertInstanceOf(IllegalStateException.class, last("failed x").field("error"));
    }

    @Test
    void moduleCallsUseTheAppsName() {
        Serlog.info("plain call");
        var app = Serlog.meta().app();
        assertEquals(app.isEmpty() ? "main" : app, last("plain call").target());
    }

    @Test
    void levelsAndCustomLevels() {
        var log = Serlog.get("calls");
        log.warn("w");
        log.fatal("c");
        log.log(Level.DEBUG, "d");
        log.log(Level.of(35), "n");
        assertEquals(
                List.of(Level.WARN, Level.FATAL, Level.DEBUG, Level.of(35)),
                List.of("w", "c", "d", "n").stream().map(m -> last(m).level()).toList());
    }

    @Test
    void argumentsAreNotWorkedOutBelowTheLevel() {
        Serlog.setLevel(Level.INFO);
        var hit = new AtomicInteger();
        Serlog.get("calls").debug("{}", Serlog.lazy(hit::incrementAndGet));
        Serlog.get("calls").atDebug().field("dump", () -> hit.incrementAndGet()).log("never");
        assertEquals(0, hit.get());
        Serlog.get("calls").info("{}", Serlog.lazy(() -> "worked"));
        last("worked");
    }

    @Test
    void bindSpansTheScope() throws Exception {
        var inner = Serlog.bind(Map.of("request", 7), () -> {
            Serlog.get("calls").atInfo().field("extra", 1).log("inside");
            return Serlog.bind(Map.of("step", 2), () -> {
                Serlog.get("calls").info("nested");
                return "done";
            });
        });
        Serlog.get("calls").info("outside");
        assertEquals("done", inner);
        assertEquals(
                List.of(Field.of("request", 7), Field.of("extra", 1)),
                last("inside").fields());
        assertEquals(
                List.of(Field.of("request", 7), Field.of("step", 2)),
                last("nested").fields());
        assertEquals(List.of(), last("outside").fields());
    }

    @Test
    void childrenAddFields() {
        Serlog.get("calls", Map.of("a", 1)).with("b", 2).info("child");
        assertEquals(List.of(Field.of("a", 1), Field.of("b", 2)), last("child").fields());
    }

    @Test
    void exceptionsCarryTheError() {
        Serlog.get("calls").exception(new IllegalArgumentException("bad input"), "parse failed");
        assertInstanceOf(IllegalArgumentException.class, last("parse failed").field("error"));
    }

    @Test
    void slf4jCallsComeThrough() {
        var log = org.slf4j.LoggerFactory.getLogger("slf");
        org.slf4j.MDC.put("request", "9");
        try {
            log.atWarn().addKeyValue("mirror", "debian").log("from {}", "slf4j");
            log.error("broke", new RuntimeException("x"));
        } finally {
            org.slf4j.MDC.clear();
        }
        var r = last("from slf4j");
        assertEquals(Level.WARN, r.level());
        assertEquals(List.of(Field.of("request", "9"), Field.of("mirror", "debian")), r.fields());
        assertEquals("slf4jCallsComeThrough", r.func());
        assertInstanceOf(RuntimeException.class, last("broke").field("error"));
    }
}
