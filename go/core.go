package serlog

import (
	"cmp"
	"context"
	"fmt"
	"io"
	"reflect"
	"runtime"
	"slices"
	"sync"
	"sync/atomic"
	"time"
)

// Go has no goroutine-local state, so records made inside serlog's own code are caught two ways: exactly, when
// they carry the context an entry hands out; and, while any sink or processor runs, by looking for serlog's
// frames on the stack.
var (
	active     atomic.Int64
	lastActive atomic.Pointer[core]
	markers    = sync.OnceValue(func() []string {
		pkg := reflect.TypeFor[core]().PkgPath()
		return []string{pkg + ".(*core).call", pkg + ".runSteps"}
	})
)

type guardKey struct{}

func suppress(ctx context.Context) bool {
	if ctx != nil {
		if v := ctx.Value(guardKey{}); v != nil {
			if c, _ := v.(*core); c != nil {
				c.suppressed.Add(1)
			}
			return true
		}
	}
	if active.Load() == 0 || !onStack() {
		return false
	}
	if c := lastActive.Load(); c != nil {
		c.suppressed.Add(1)
	}
	return true
}

func onStack() bool {
	var pcs [64]uintptr
	frames := runtime.CallersFrames(pcs[:runtime.Callers(3, pcs[:])])
	marks := markers()
	for {
		f, more := frames.Next()
		if slices.Contains(marks, f.Function) {
			return true
		}
		if !more {
			return false
		}
	}
}

func now() float64 { return float64(time.Now().UnixNano()) / 1e9 }

// core is the sink, its look, its policy and its health, shared by the callers and the queue.
type core struct {
	name       string
	kind       string
	sink       Sink
	layout     Layout
	cacheable  bool
	theme      *Theme
	panics     bool
	alarm      Alarm
	retries    int
	br         *breaker
	dropped    atomic.Int64
	suppressed atomic.Int64
	mu         sync.Mutex
}

//go:noinline
func (c *core) call(fn func() error) (err error) {
	active.Add(1)
	lastActive.Store(c)
	defer active.Add(-1)
	c.mu.Lock()
	defer c.mu.Unlock()
	defer func() {
		if p := recover(); p != nil {
			err = fmt.Errorf("panicked: %v", p)
		}
	}()
	return fn()
}

func (c *core) up() bool { return c.br.up() }

func (c *core) tell() tell {
	if c.alarm != nil {
		return report
	}
	return silent
}

func (c *core) entry(r *Record, cache map[cacheKey]string) *Entry {
	e := &Entry{Record: r, layout: c.layout, theme: c.theme, owner: c}
	if c.cacheable {
		e.cache = cache
	}
	return e
}

func (c *core) batch(es []*Entry) error {
	return c.call(func() error {
		if len(es) == 1 {
			return c.sink.Emit(es[0])
		}
		if b, ok := c.sink.(batcher); ok {
			return b.EmitBatch(es)
		}
		for _, e := range es {
			if err := c.sink.Emit(e); err != nil {
				return err
			}
		}
		return nil
	})
}

// write writes in the caller, retrying without pause; the panic policy panics, the others count or tell.
func (c *core) write(e *Entry) {
	if !c.panics && !c.up() && !c.br.admit(now()) {
		return
	}
	for n := 0; ; n++ {
		err := c.batch([]*Entry{e})
		if err == nil {
			break
		}
		if retryable(err) && n < c.retries {
			continue
		}
		if c.panics {
			panic(&SinkError{c.name, err})
		}
		c.failed(err, c.tell())
		return
	}
	c.passed()
}

func (c *core) attempt(es []*Entry, pause bool) error {
	for n := 0; ; n++ {
		err := c.batch(es)
		if err == nil || !retryable(err) || n >= c.retries {
			return err
		}
		if pause {
			time.Sleep(time.Duration(1<<min(n, 5)) * time.Second)
		}
	}
}

// drain writes a batch from the queue, waiting out failures until it goes through or is refused.
func (c *core) drain(buf []*Record) {
	for len(buf) > 0 {
		if !c.up() {
			time.Sleep(time.Duration(c.br.wait(now()) * 1e9))
			c.br.admit(now())
		}
		probing := !c.up()
		es := make([]*Entry, len(buf))
		for i, r := range buf {
			es[i] = c.entry(r, nil)
		}
		err := c.attempt(es, !probing)
		if err == nil {
			c.passed()
			return
		}
		if c.panics {
			crash(fmt.Sprintf("sink `%s` failed: %v", c.name, err))
		}
		c.failed(err, c.tell())
		if !retryable(err) {
			c.dropped.Add(int64(len(buf)))
			return
		}
	}
}

// deliver writes an alarm record straight to the sink; a failure rides in the incident being delivered.
func (c *core) deliver(r *Record) error {
	if !c.up() && !c.br.admit(now()) {
		return fmt.Errorf("down")
	}
	err := c.batch([]*Entry{c.entry(r, nil)})
	if err == nil {
		err = c.flush()
	}
	if err != nil {
		c.failed(err, folded)
		return err
	}
	c.passed()
	return nil
}

func (c *core) flush() error {
	f, ok := c.sink.(flusher)
	if !ok {
		return nil
	}
	return c.call(f.Flush)
}

func (c *core) resting() bool { return !c.up() && c.br.resting(now()) }

func (c *core) passed() {
	if c.up() {
		return
	}
	if p := c.br.passed(); p != "" {
		c.notify(p)
	}
}

func (c *core) failed(err error, t tell) {
	remind := 1e18
	if c.alarm != nil {
		_, r := settings(resolve(c.alarm))
		remind = r.Seconds()
	}
	if p := c.br.failed(now(), message(err), remind, t); p != "" {
		c.notify(p)
	}
}

func (c *core) notify(p Phase) {
	if c.alarm == nil {
		return
	}
	_, since, _, skippedNow, err := c.br.snapshot()
	i := NewIncident(c.name, c.kind, p, err)
	i.Skipped, i.Dropped, i.Since = skippedNow, c.dropped.Load(), since
	post(c.alarm, i)
}

// Status is a slot's state, for monitoring.
type Status struct {
	Name       string
	Kind       string
	Health     Health
	Skipped    int64
	Dropped    int64
	Suppressed int64
	Since      time.Time
	Error      string
}

func (c *core) status() Status {
	h, since, skipped, _, err := c.br.snapshot()
	return Status{c.name, c.kind, h, skipped, c.dropped.Load(), c.suppressed.Load(), since, err}
}

// lane is a slot's queue and the goroutine draining it, made when the first record arrives.
type lane struct {
	shape  Queue
	core   *core
	ch     chan *Record
	asks   chan chan struct{}
	quit   chan struct{}
	start  sync.Once
	worker atomic.Bool
}

func newLane(q Queue, c *core) *lane {
	return &lane{shape: q, core: c, ch: make(chan *Record, q.Size), asks: make(chan chan struct{}), quit: make(chan struct{})}
}

func (l *lane) push(r *Record) {
	l.start.Do(func() {
		l.worker.Store(true)
		go l.run()
	})
	select {
	case l.ch <- r:
		return
	default:
	}
	if l.shape.Full != "block" {
		l.core.dropped.Add(1)
		return
	}
	for l.core.up() {
		select {
		case l.ch <- r:
			return
		case <-time.After(50 * time.Millisecond):
		}
	}
	l.core.dropped.Add(1)
}

func (l *lane) run() {
	batch := make([]*Record, 0, min(l.shape.Batch, 4096))
	var waiting []chan struct{}
	for {
		if len(l.ch) == 0 {
			for _, w := range waiting {
				close(w)
			}
			waiting = waiting[:0]
		}
		select {
		case r := <-l.ch:
			batch = append(batch, r)
		case w := <-l.asks:
			waiting = append(waiting, w)
			continue
		case <-l.quit:
			return
		}
		timer := time.NewTimer(l.shape.Wait)
	gather:
		for len(batch) < l.shape.Batch {
			if len(waiting) > 0 {
				select {
				case r := <-l.ch:
					batch = append(batch, r)
					continue
				default:
					break gather
				}
			}
			select {
			case r := <-l.ch:
				batch = append(batch, r)
			case w := <-l.asks:
				waiting = append(waiting, w)
			case <-timer.C:
				break gather
			}
		}
		timer.Stop()
		l.core.drain(batch)
		clear(batch)
		batch = batch[:0]
	}
}

// flush waits until what is queued has been written, unless the sink is down or the time is up.
func (l *lane) flush(within time.Duration) {
	if !l.worker.Load() || !l.core.up() {
		return
	}
	done := make(chan struct{})
	timeout := time.After(within)
	select {
	case l.asks <- done:
	case <-timeout:
		return
	}
	select {
	case <-done:
	case <-timeout:
	}
}

// live is a registered slot: its settings, the core both the callers and the queue use, and its queue.
type live struct {
	core      *core
	slot      Slot
	filter    *Filter
	layout    Layout
	alarmOnly bool
	queue     *Queue
	claim     string
	lane      *lane
	caller    bool
}

func kindOf(s Sink) string {
	if k, ok := inner(s).(kinder); ok {
		return k.Kind()
	}
	return "custom"
}

func newLive(s Sink, name string, named map[string]Alarm) (*live, error) {
	sl := slotOf(s)
	in := inner(s)
	bad := func(err error) error { return &ConfigError{Msg: fmt.Sprintf("sink `%s`: %v", name, err), Err: err} }
	l := &live{slot: sl, alarmOnly: sl.AlarmOnly}
	if sl.Level != 0 && sl.Filter != "" {
		return nil, configErr("sink `%s`: give `level` or `filter`, not both", name)
	}
	if sl.Level != 0 {
		l.filter = LevelFilter(sl.Level)
	} else if sl.Filter != "" {
		f, err := ParseFilter(sl.Filter)
		if err != nil {
			return nil, bad(err)
		}
		l.filter = f
	}
	lay, err := buildLayout(&sl)
	if err != nil {
		return nil, err
	}
	l.layout = lay
	if lay == nil {
		lay = Inherit
		if d, ok := in.(defLayout); ok {
			lay = d.DefaultLayout()
		}
	}
	var theme *Theme
	tty := false
	if t, ok := in.(ttyer); ok {
		tty = t.TTY()
	}
	on, err := colourOn(sl.Colour, tty)
	if err != nil {
		return nil, bad(err)
	}
	if on {
		th := sl.Colours
		if th == nil {
			th = Theme{}
		}
		theme = &th
	}
	if sl.Full != "" && sl.Full != "drop" && sl.Full != "block" {
		return nil, configErr("sink `%s`: `full` is drop or block, not `%s`", name, sl.Full)
	}
	l.queue = shape(in, sl)
	policy, alarm, err := onFail(sl, named)
	if err != nil {
		return nil, err
	}
	retries := 0
	switch {
	case sl.Retries == NoRetries:
	case sl.Retries > 0:
		retries = sl.Retries
	case l.queue != nil:
		retries = 3
	}
	probe := cmp.Or(sl.Probe, 5*time.Second)
	c := &core{
		name: name, kind: kindOf(s), sink: in, layout: lay, cacheable: reflect.TypeOf(lay).Comparable(),
		theme: theme, panics: policy == "panic", alarm: alarm, retries: retries, br: newBreaker(probe.Seconds(), now()),
	}
	l.core = c
	if cl, ok := in.(claimer); ok {
		l.claim = cl.Claim()
	}
	if l.queue != nil {
		l.lane = newLane(*l.queue, c)
	}
	l.caller = showsCaller(lay)
	return l, nil
}

func shape(s Sink, sl Slot) *Queue {
	var base *Queue
	if d, ok := s.(defQueue); ok {
		q := d.DefaultQueue()
		base = &q
	}
	if sl.Queue == Direct {
		return nil
	}
	if sl.Queue == 0 && sl.Batch == 0 && sl.Wait == 0 && sl.Full == "" {
		return base
	}
	q := Queue{Size: 1024, Batch: 1000, Wait: time.Second, Full: "drop"}
	if base != nil {
		q = *base
	}
	q.Size = cmp.Or(sl.Queue, q.Size)
	q.Batch = cmp.Or(sl.Batch, q.Batch)
	q.Wait = cmp.Or(sl.Wait, q.Wait)
	q.Full = or(sl.Full, q.Full)
	return &q
}

func onFail(sl Slot, named map[string]Alarm) (string, Alarm, error) {
	if sl.Alarm != nil {
		return "", sl.Alarm, nil
	}
	switch sl.OnFail {
	case "", "default":
		return "", defaultAlarm, nil
	case "panic", "ignore":
		return sl.OnFail, nil, nil
	}
	if a, ok := named[sl.OnFail]; ok {
		return "", a, nil
	}
	return "", nil, configErr("`on_fail` names `%s`, which is not in `alarms`", sl.OnFail)
}

func (l *live) name() string { return l.core.name }

func (l *live) emit(r *Record, cache map[cacheKey]string) {
	if l.filter != nil && !l.filter.Allows(r.Level, r.Target) {
		return
	}
	if e, ok := l.core.sink.(enabler); ok && !e.Enabled(r.Level, r.Target) {
		return
	}
	rec := r
	if len(l.slot.Process) > 0 {
		if rec = runSteps(l.slot.Process, r); rec == nil {
			return
		}
	}
	if l.lane != nil {
		l.lane.push(rec)
		return
	}
	if rec != r {
		cache = nil
	}
	l.core.write(l.core.entry(rec, cache))
}

// retire writes what the queue holds, stops its goroutine and closes the sink if it is an io.Closer.
func (l *live) retire() {
	l.flush(5 * time.Second)
	if l.lane != nil {
		close(l.lane.quit)
	}
	if c, ok := l.core.sink.(io.Closer); ok {
		l.core.call(c.Close)
	}
}

func (l *live) flush(within time.Duration) {
	if l.lane != nil {
		l.lane.flush(within)
		return
	}
	if l.core.up() {
		if err := l.core.flush(); err != nil {
			l.core.failed(err, l.core.tell())
		}
	}
}
