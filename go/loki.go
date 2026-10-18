package serlog

import (
	"bytes"
	"cmp"
	"compress/gzip"
	"context"
	"fmt"
	"maps"
	"net/http"
	"slices"
	"strconv"
	"strings"
	"time"
)

// Loki pushes to Grafana Loki's push API, which Alloy's `loki.source.api` accepts as well: gzipped JSON, one
// stream per level. Lines are the slot's layout, `{msg}` unless set; `target`, `file` and the fields travel as
// structured metadata. Network errors, 429 and 5xx are worth retrying; other 4xx refuse the batch.
type Loki struct {
	Slot
	URL     string
	Labels  map[string]string
	Timeout time.Duration
	Connect bool
	client  *http.Client
}

// Kind is "loki".
func (l *Loki) Kind() string { return "loki" }

// DefaultLayout is `{msg}`.
func (l *Loki) DefaultLayout() Layout { return MustPattern("{msg}") }

// DefaultQueue holds 8192 records, written 1000 at a time or a second after the first.
func (l *Loki) DefaultQueue() Queue {
	return Queue{Size: 8192, Batch: 1000, Wait: time.Second, Full: "drop"}
}

func (l *Loki) timeout() time.Duration { return cmp.Or(l.Timeout, 10*time.Second) }

func (l *Loki) http() *http.Client {
	if l.client == nil {
		l.client = &http.Client{Timeout: l.timeout()}
	}
	return l.client
}

func object(pairs [][2]string) string {
	parts := make([]string, len(pairs))
	for i, p := range pairs {
		parts[i] = quote(p[0]) + ":" + quote(p[1])
	}
	return "{" + strings.Join(parts, ",") + "}"
}

// Body is the push request body: labels per stream, structured metadata per line.
func (l *Loki) Body(es []*Entry) string {
	m := meta()
	streams := map[Level][]*Entry{}
	for _, e := range es {
		streams[e.Record.Level] = append(streams[e.Record.Level], e)
	}
	var out []string
	for _, level := range slices.Sorted(maps.Keys(streams)) {
		var labels [][2]string
		for _, id := range [][2]string{{"app", m.App}, {"env", m.Env}, {"instance", m.Instance}} {
			if id[1] != "" {
				labels = append(labels, id)
			}
		}
		labels = append(labels, [2]string{"level", level.String()})
		for _, k := range slices.Sorted(maps.Keys(l.Labels)) {
			labels = append(labels, [2]string{k, l.Labels[k]})
		}
		var values []string
		for _, e := range streams[level] {
			r := e.Record
			extra := [][2]string{{"target", r.Target}}
			if r.File != "" && r.Line > 0 {
				extra = append(extra, [2]string{"file", r.File + ":" + strconv.Itoa(r.Line)})
			}
			for _, f := range flat(r.Fields) {
				extra = append(extra, [2]string{f.Key, show(f.Value)})
			}
			values = append(values, fmt.Sprintf(`["%d",%s,%s]`, r.Time.UnixNano(), quote(e.Line()), object(extra)))
		}
		out = append(out, `{"stream":`+object(labels)+`,"values":[`+strings.Join(values, ",")+"]}")
	}
	return `{"streams":[` + strings.Join(out, ",") + "]}"
}

// Emit pushes one entry.
func (l *Loki) Emit(e *Entry) error { return l.EmitBatch([]*Entry{e}) }

// EmitBatch pushes a batch in one request.
func (l *Loki) EmitBatch(es []*Entry) error {
	var buf bytes.Buffer
	zw := gzip.NewWriter(&buf)
	if _, err := zw.Write([]byte(l.Body(es))); err != nil {
		return err
	}
	if err := zw.Close(); err != nil {
		return err
	}
	req, err := http.NewRequestWithContext(context.Background(), http.MethodPost, l.URL, &buf)
	if err != nil {
		return err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Content-Encoding", "gzip")
	resp, err := l.http().Do(req)
	if err != nil {
		return err
	}
	resp.Body.Close()
	switch s := resp.StatusCode; {
	case s/100 == 2:
		return nil
	case s >= 400 && s < 500 && s != 429:
		return fmt.Errorf("loki answered %d: %w", s, ErrRejected)
	default:
		return fmt.Errorf("loki answered %d", s)
	}
}

// Ready checks the URL and, with Connect, asks `GET /ready`.
func (l *Loki) Ready() error {
	if !strings.HasPrefix(l.URL, "http://") && !strings.HasPrefix(l.URL, "https://") {
		return configErr("loki url `%s` is not http or https", l.URL)
	}
	if !l.Connect {
		return nil
	}
	base, _, ok := strings.Cut(l.URL, "/loki/api/")
	if !ok {
		base = strings.TrimRight(l.URL, "/")
	}
	resp, err := l.http().Get(base + "/ready")
	if err != nil {
		return err
	}
	resp.Body.Close()
	if resp.StatusCode/100 != 2 {
		return fmt.Errorf("loki /ready answered %d", resp.StatusCode)
	}
	return nil
}

// Options describes the sink for export.
func (l *Loki) Options() map[string]any {
	o := map[string]any{"url": l.URL}
	if len(l.Labels) > 0 {
		o["labels"] = maps.Clone(l.Labels)
	}
	if l.timeout() != 10*time.Second {
		o["timeout"] = showTime(l.timeout())
	}
	if l.Connect {
		o["connect"] = true
	}
	return o
}
