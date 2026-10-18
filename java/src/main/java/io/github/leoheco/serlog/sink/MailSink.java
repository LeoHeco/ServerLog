package io.github.leoheco.serlog.sink;

import io.github.leoheco.serlog.Entry;
import io.github.leoheco.serlog.Level;
import io.github.leoheco.serlog.Queued;
import io.github.leoheco.serlog.SerlogException;
import io.github.leoheco.serlog.SinkBase;
import io.github.leoheco.serlog.internal.Hub;
import io.github.leoheco.serlog.internal.Units;
import io.github.leoheco.serlog.layout.Clock;
import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Optional;
import java.util.Properties;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import java.util.stream.Collectors;
import java.util.stream.Stream;

/// Email for serious records: a batch goes out as one message, plain text and HTML. Only `error` and above unless
/// the slot sets a level or filter. Needs `org.eclipse.angus:angus-mail`.
public final class MailSink extends SinkBase<MailSink> {
    /// How the connection is secured.
    public enum Security {
        TLS,
        STARTTLS,
        NONE;

        /// A setting by its config name.
        public static Security parse(String name) {
            return valueOf(name.strip().toUpperCase(Locale.ROOT));
        }

        /// The config name.
        public String id() {
            return name().toLowerCase(Locale.ROOT);
        }
    }

    /// Subject, plain text and HTML of one message.
    public record Message(String subject, String text, String html) {}

    private static final Pattern ADDRESS = Pattern.compile("[^\\s@<>]+@[^\\s@<>]+");
    private static final Pattern FILL = Pattern.compile("\\{\\{(\\w+)}}");

    private static final class Page {
        static final String HTML = load();

        static String load() {
            try (var in = MailSink.class.getResourceAsStream("mail.html")) {
                return new String(in.readAllBytes(), StandardCharsets.UTF_8);
            } catch (IOException e) {
                throw new UncheckedIOException(e);
            }
        }
    }

    private final String host;
    private List<String> to = List.of();
    private Integer port;
    private Security security = Security.TLS;
    private String user;
    private String pass;
    private String from;
    private Duration timeout = Duration.ofSeconds(10);
    private boolean connect;

    /// Sends through `host`.
    public MailSink(String host) {
        this.host = host;
    }

    /// The recipients.
    public MailSink to(String... to) {
        this.to = List.of(to);
        return this;
    }

    /// The port; 465 for TLS, 587 for STARTTLS and 25 otherwise by default.
    public MailSink port(int port) {
        this.port = port;
        return this;
    }

    /// How the connection is secured, TLS by default.
    public MailSink security(Security security) {
        this.security = security;
        return this;
    }

    /// The login.
    public MailSink login(String user, String pass) {
        this.user = user;
        this.pass = pass;
        return this;
    }

    /// The sender; the user by default.
    public MailSink from(String from) {
        this.from = from;
        return this;
    }

    /// How long the server may take, 10 s by default.
    public MailSink timeout(Duration timeout) {
        this.timeout = timeout;
        return this;
    }

    /// Whether to log in at start-up.
    public MailSink connect(boolean connect) {
        this.connect = connect;
        return this;
    }

    /// The page every message is filled into.
    public static String page() {
        return Page.HTML;
    }

    @Override
    public String kind() {
        return "mail";
    }

    private String sender() {
        return from != null ? from : user != null ? user : "";
    }

    @Override
    public boolean enabled(Level level, String target) {
        return slot().containsKey("level") || slot().containsKey("filter") || level.atLeast(Level.ERROR);
    }

    private static String html(String s) {
        var out = new StringBuilder(s.length());
        for (var c : s.toCharArray())
            switch (c) {
                case '&' -> out.append("&amp;");
                case '<' -> out.append("&lt;");
                case '>' -> out.append("&gt;");
                case '"' -> out.append("&quot;");
                case '\'' -> out.append("&#39;");
                default -> out.append(c);
            }
        return out.toString();
    }

    /// Subject, plain text and HTML for one batch.
    public Message render(List<Entry> entries) {
        var m = Hub.meta();
        var top = entries.stream()
                .map(e -> e.record().level())
                .max(Level::compareTo)
                .orElse(Level.ERROR);
        var n = entries.size();
        var rows = entries.stream()
                .map(e -> {
                    var r = e.record();
                    return "<tr><td>" + Clock.strftime(r.time(), "%F %T") + "</td><td class=\""
                            + r.level().id() + "\">" + r.level().label(Hub.compat()) + "</td><td>" + html(r.target())
                            + "</td><td>" + html(r.msg()) + "</td></tr>";
                })
                .collect(Collectors.joining());
        var fill = Map.of(
                "app",
                html(m.app()),
                "env",
                html(m.env()),
                "instance",
                html(m.instance()),
                "level",
                top.id(),
                "count",
                String.valueOf(n),
                "rows",
                rows);
        return new Message(
                "[" + m.app() + "] " + n + " " + top.label(Hub.compat()) + " record" + (n == 1 ? "" : "s"),
                entries.stream().map(e -> e.line() + "\n").collect(Collectors.joining()),
                FILL.matcher(Page.HTML)
                        .replaceAll(x -> Matcher.quoteReplacement(fill.getOrDefault(x.group(1), x.group()))));
    }

    private Properties props() {
        var p = new Properties();
        var ms = String.valueOf(timeout.toMillis());
        p.put("mail.smtp.host", host);
        p.put(
                "mail.smtp.port",
                String.valueOf(
                        port != null
                                ? port
                                : switch (security) {
                                    case TLS -> 465;
                                    case STARTTLS -> 587;
                                    case NONE -> 25;
                                }));
        p.put("mail.smtp.connectiontimeout", ms);
        p.put("mail.smtp.timeout", ms);
        p.put("mail.smtp.writetimeout", ms);
        p.put("mail.smtp.ssl.enable", String.valueOf(security == Security.TLS));
        p.put("mail.smtp.starttls.enable", String.valueOf(security == Security.STARTTLS));
        p.put("mail.smtp.starttls.required", String.valueOf(security == Security.STARTTLS));
        p.put("mail.smtp.auth", String.valueOf(user != null && pass != null));
        return p;
    }

    /// The SMTP calls, kept apart so the rest loads without Angus Mail.
    private static final class Smtp {
        static void send(Properties props, String from, List<String> to, Message m, String user, String pass)
                throws Exception {
            var msg = new jakarta.mail.internet.MimeMessage(jakarta.mail.Session.getInstance(props));
            msg.setFrom(new jakarta.mail.internet.InternetAddress(from));
            msg.setRecipients(jakarta.mail.Message.RecipientType.TO, String.join(", ", to));
            msg.setSubject(m.subject(), "UTF-8");
            var body = new jakarta.mail.internet.MimeMultipart("alternative");
            var text = new jakarta.mail.internet.MimeBodyPart();
            text.setText(m.text(), "UTF-8");
            var page = new jakarta.mail.internet.MimeBodyPart();
            page.setContent(m.html(), "text/html; charset=UTF-8");
            body.addBodyPart(text);
            body.addBodyPart(page);
            msg.setContent(body);
            try {
                if (user != null && pass != null) jakarta.mail.Transport.send(msg, user, pass);
                else jakarta.mail.Transport.send(msg);
            } catch (org.eclipse.angus.mail.smtp.SMTPSendFailedException e) {
                if (e.getReturnCode() >= 500) throw new SerlogException.Rejected("mail refused: " + e.getReturnCode());
                throw e;
            }
        }

        static void login(Properties props, String host, String user, String pass) throws Exception {
            try (var t = jakarta.mail.Session.getInstance(props).getTransport("smtp")) {
                t.connect(host, user, pass);
            }
        }
    }

    @Override
    public void emit(Entry entry) throws Exception {
        emitBatch(List.of(entry));
    }

    @Override
    public void emitBatch(List<Entry> entries) throws Exception {
        if (entries.isEmpty()) return;
        Smtp.send(props(), sender(), to, render(entries), user, pass);
    }

    @Override
    public void ready() throws Exception {
        if (to.isEmpty()) throw new SerlogException.Config("mail: no recipients");
        if (sender().isEmpty()) throw new SerlogException.Config("mail: no sender");
        for (var a : Stream.concat(Stream.of(sender()), to.stream()).toList()) {
            var bare = a.replaceAll("^.*<(.*)>$", "$1");
            if (!ADDRESS.matcher(bare).matches()) throw new SerlogException.Config("mail: bad address `" + a + "`");
        }
        if (user != null && pass == null) throw new SerlogException.Config("mail: a user needs a password");
        if (connect) Smtp.login(props(), host, user, pass);
    }

    @Override
    public Optional<Queued> queued() {
        return Optional.of(Queued.of(1024).every(1000, Duration.ofSeconds(60)));
    }

    @Override
    public Optional<Map<String, Object>> options() {
        var o = new LinkedHashMap<String, Object>();
        o.put("host", host);
        if (port != null) o.put("port", port);
        if (security != Security.TLS) o.put("security", security.id());
        if (user != null) {
            o.put("user", user);
            o.put("pass", "***");
        }
        if (from != null) o.put("from", from);
        o.put("to", to);
        if (!timeout.equals(Duration.ofSeconds(10))) o.put("timeout", Units.showTime(timeout));
        if (connect) o.put("connect", true);
        return Optional.of(o);
    }
}
