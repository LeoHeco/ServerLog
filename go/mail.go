package serlog

import (
	"cmp"
	"crypto/tls"
	_ "embed"
	"errors"
	"fmt"
	"html"
	"mime"
	"mime/multipart"
	"mime/quotedprintable"
	"net"
	"net/mail"
	"net/smtp"
	"net/textproto"
	"regexp"
	"strconv"
	"strings"
	"time"
)

//go:embed mail.html
var mailPage string

// MailPage is the page every message is filled into.
func MailPage() string { return mailPage }

var fill = regexp.MustCompile(`\{\{(\w+)\}\}`)

// Mail sends email for serious records: a batch goes out as one message, plain text and HTML. Only error and
// above unless the slot sets a level or filter. Security is "tls" (the default), "starttls" or "none"; From
// defaults to User.
type Mail struct {
	Slot
	Host     string
	Port     int
	Security string
	User     string
	Pass     string
	From     string
	To       []string
	Timeout  time.Duration
	Connect  bool
}

// Message is the subject, plain text and HTML of one message.
type Message struct {
	Subject, Text, HTML string
}

// Kind is "mail".
func (m *Mail) Kind() string { return "mail" }

// DefaultQueue holds 1024 records, written 1000 at a time or a minute after the first.
func (m *Mail) DefaultQueue() Queue {
	return Queue{Size: 1024, Batch: 1000, Wait: time.Minute, Full: "drop"}
}

// Enabled lets only error and above through unless the slot sets a level or filter.
func (m *Mail) Enabled(l Level, _ string) bool {
	return m.Slot.Level != 0 || m.Slot.Filter != "" || l >= LevelError
}

func (m *Mail) sender() string { return or(m.From, m.User) }

func (m *Mail) security() string { return or(m.Security, "tls") }

func (m *Mail) port() int {
	if m.Port != 0 {
		return m.Port
	}
	switch m.security() {
	case "starttls":
		return 587
	case "none":
		return 25
	}
	return 465
}

// Render is the subject, plain text and HTML for one batch.
func (m *Mail) Render(es []*Entry) Message {
	id := meta()
	top := LevelTrace
	var rows, text strings.Builder
	for _, e := range es {
		r := e.Record
		top = max(top, r.Level)
		fmt.Fprintf(&rows, `<tr><td>%s</td><td class="%s">%s</td><td>%s</td><td>%s</td></tr>`,
			strftime(r.Time.Local(), "%F %T"), r.Level, r.Level.Label(compat()), html.EscapeString(r.Target), html.EscapeString(r.Msg))
		text.WriteString(e.Line() + "\n")
	}
	n := len(es)
	values := map[string]string{
		"app": html.EscapeString(id.App), "env": html.EscapeString(id.Env), "instance": html.EscapeString(id.Instance),
		"level": top.String(), "count": strconv.Itoa(n), "rows": rows.String(),
	}
	plural := "s"
	if n == 1 {
		plural = ""
	}
	return Message{
		Subject: fmt.Sprintf("[%s] %d %s record%s", id.App, n, top.Label(compat()), plural),
		Text:    text.String(),
		HTML: fill.ReplaceAllStringFunc(mailPage, func(s string) string {
			if v, ok := values[s[2:len(s)-2]]; ok {
				return v
			}
			return s
		}),
	}
}

func (m *Mail) dial() (*smtp.Client, error) {
	addr := net.JoinHostPort(m.Host, strconv.Itoa(m.port()))
	d := &net.Dialer{Timeout: cmp.Or(m.Timeout, 10*time.Second)}
	var conn net.Conn
	var err error
	if m.security() == "tls" {
		conn, err = tls.DialWithDialer(d, "tcp", addr, &tls.Config{ServerName: m.Host})
	} else {
		conn, err = d.Dial("tcp", addr)
	}
	if err != nil {
		return nil, err
	}
	conn.SetDeadline(time.Now().Add(cmp.Or(m.Timeout, 10*time.Second)))
	c, err := smtp.NewClient(conn, m.Host)
	if err != nil {
		conn.Close()
		return nil, err
	}
	if m.security() == "starttls" {
		if err := c.StartTLS(&tls.Config{ServerName: m.Host}); err != nil {
			c.Close()
			return nil, err
		}
	}
	if m.User != "" {
		if err := c.Auth(smtp.PlainAuth("", m.User, m.Pass, m.Host)); err != nil {
			c.Close()
			return nil, err
		}
	}
	return c, nil
}

func (m *Mail) compose(msg Message) (string, error) {
	var body strings.Builder
	w := multipart.NewWriter(&body)
	for _, part := range []struct{ kind, text string }{{"text/plain", msg.Text}, {"text/html", msg.HTML}} {
		pw, err := w.CreatePart(textproto.MIMEHeader{
			"Content-Type":              {part.kind + "; charset=utf-8"},
			"Content-Transfer-Encoding": {"quoted-printable"},
		})
		if err != nil {
			return "", err
		}
		qw := quotedprintable.NewWriter(pw)
		qw.Write([]byte(part.text))
		qw.Close()
	}
	w.Close()
	head := fmt.Sprintf("From: %s\r\nTo: %s\r\nSubject: %s\r\nDate: %s\r\nMIME-Version: 1.0\r\nContent-Type: multipart/alternative; boundary=%s\r\n\r\n",
		m.sender(), strings.Join(m.To, ", "), mime.QEncoding.Encode("utf-8", msg.Subject), time.Now().Format(time.RFC1123Z), w.Boundary())
	return head + body.String(), nil
}

// Emit sends one entry as a message.
func (m *Mail) Emit(e *Entry) error { return m.EmitBatch([]*Entry{e}) }

// EmitBatch sends a batch as one message.
func (m *Mail) EmitBatch(es []*Entry) error {
	text, err := m.compose(m.Render(es))
	if err != nil {
		return err
	}
	c, err := m.dial()
	if err != nil {
		return err
	}
	defer c.Close()
	refused := func(err error) error {
		var te *textproto.Error
		if errors.As(err, &te) && te.Code >= 500 {
			return fmt.Errorf("mail refused: %d: %w", te.Code, ErrRejected)
		}
		return err
	}
	if err := c.Mail(m.sender()); err != nil {
		return refused(err)
	}
	for _, to := range m.To {
		if err := c.Rcpt(to); err != nil {
			return refused(err)
		}
	}
	w, err := c.Data()
	if err != nil {
		return refused(err)
	}
	if _, err := w.Write([]byte(text)); err != nil {
		return err
	}
	if err := w.Close(); err != nil {
		return refused(err)
	}
	return c.Quit()
}

// Ready checks the addresses and, with Connect, logs in.
func (m *Mail) Ready() error {
	if len(m.To) == 0 {
		return configErr("mail: no recipients")
	}
	if m.sender() == "" {
		return configErr("mail: no sender")
	}
	for _, a := range append([]string{m.sender()}, m.To...) {
		if _, err := mail.ParseAddress(a); err != nil {
			return configErr("mail: bad address `%s`", a)
		}
	}
	if m.User != "" && m.Pass == "" {
		return configErr("mail: a user needs a password")
	}
	switch m.security() {
	case "tls", "starttls", "none":
	default:
		return configErr("mail: `security` is tls, starttls or none, not `%s`", m.Security)
	}
	if m.Connect {
		c, err := m.dial()
		if err != nil {
			return err
		}
		return c.Quit()
	}
	return nil
}

// Options describes the sink for export; the password comes out as `***`.
func (m *Mail) Options() map[string]any {
	o := map[string]any{"host": m.Host, "to": m.To}
	if m.Port != 0 {
		o["port"] = m.Port
	}
	if m.security() != "tls" {
		o["security"] = m.security()
	}
	if m.User != "" {
		o["user"], o["pass"] = m.User, "***"
	}
	if m.From != "" {
		o["from"] = m.From
	}
	if t := cmp.Or(m.Timeout, 10*time.Second); t != 10*time.Second {
		o["timeout"] = showTime(t)
	}
	if m.Connect {
		o["connect"] = true
	}
	return o
}
