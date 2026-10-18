package io.github.leoheco.serlog;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertSame;

import io.github.leoheco.serlog.process.Dedupe;
import io.github.leoheco.serlog.process.Fields;
import io.github.leoheco.serlog.process.Limit;
import io.github.leoheco.serlog.process.Match;
import io.github.leoheco.serlog.process.Redact;
import io.github.leoheco.serlog.process.Sample;
import io.github.leoheco.serlog.process.Sanitise;
import java.time.Duration;
import java.util.List;
import java.util.Map;
import java.util.stream.IntStream;
import org.junit.jupiter.api.Test;

/// The built-in processors, run on single records.
class ProcessorsTest {
    private static LogRecord rec(String msg, Field... fields) {
        return LogRecord.of(Level.INFO, "t", msg, List.of(fields));
    }

    @Test
    void redactsKeysPathsAndPatterns() {
        var r = rec(
                "call 13800138000 now",
                Field.of("pass", "hunter2"),
                Field.of("req", Map.of("headers", Map.of("authorization", "Bearer x", "accept", "*/*"))),
                Field.of("user", "ann"));
        var out = Redact.keys("pass")
                .paths("req.headers.authorization")
                .patterns("\\d{11}")
                .process(r);
        assertEquals("***", out.field("pass"));
        assertEquals("ann", out.field("user"));
        assertEquals(Map.of("headers", Map.of("authorization", "***", "accept", "*/*")), out.field("req"));
        assertEquals("call *** now", out.msg());
        assertEquals("hunter2", r.field("pass"));
    }

    @Test
    void leavesCleanRecordsAlone() {
        var r = rec("fine", Field.of("n", 1));
        assertSame(r, Redact.keys("pass").process(r));
        assertSame(r, new Sanitise().process(r));
    }

    @Test
    void sanitisesLineBreaks() {
        var out = new Sanitise().process(rec("x\r\ny\u0007", Field.of("who", "a\nb")));
        assertEquals("x\\r\\ny\\u{0007}", out.msg());
        assertEquals("a\\nb", out.field("who"));
    }

    @Test
    void addsMatchesAndExcludes() {
        assertEquals("cn", new Fields(Map.of("region", "cn")).process(rec("x")).field("region"));
        var r = rec("x", Field.of("mirror", "debian")).withLevel(Level.WARN).withTags(List.of("audit"));
        var all = new Match(
                List.of("audit"), Match.fields(Map.of("mirror", "debian")).fields(), Level.WARN, false);
        assertSame(r, all.process(r));
        assertNull(Match.tags("other").process(r));
        assertNull(Match.tags("audit").negate().process(r));
        assertSame(r, Match.level(Level.ERROR).negate().process(r));
    }

    @Test
    void samplesLimitsAndDedupes() throws InterruptedException {
        var s = new Sample(2, 3, Duration.ofMinutes(1));
        assertEquals(
                List.of(true, true, false, false, true, false, false, true),
                IntStream.range(0, 8)
                        .mapToObj(_ -> s.process(rec("same")) != null)
                        .toList());
        var lim = new Limit(0, 3);
        assertEquals(
                3,
                IntStream.range(0, 5).filter(_ -> lim.process(rec("x")) != null).count());
        var d = new Dedupe(Duration.ofMillis(200));
        assertNotNull(d.process(rec("again")));
        assertNull(d.process(rec("again")));
        assertNull(d.process(rec("again")));
        Thread.sleep(250);
        assertEquals(2L, d.process(rec("again")).field("repeated"));
    }
}
