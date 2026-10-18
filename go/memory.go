package serlog

import (
	"cmp"
	"slices"
	"sync"
)

// Memory keeps the newest Size records and their lines, 1000 by default; for tests and for looking back.
type Memory struct {
	Slot
	Size  int
	mu    sync.Mutex
	kept  []*Record
	lines []string
}

// Kind is "memory".
func (m *Memory) Kind() string { return "memory" }

// Emit keeps the entry.
func (m *Memory) Emit(e *Entry) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.kept = append(m.kept, e.Record)
	m.lines = append(m.lines, e.Line())
	if n := len(m.kept) - max(cmp.Or(m.Size, 1000), 1); n > 0 {
		m.kept, m.lines = slices.Delete(m.kept, 0, n), slices.Delete(m.lines, 0, n)
	}
	return nil
}

// Records is the records kept, oldest first.
func (m *Memory) Records() []*Record {
	m.mu.Lock()
	defer m.mu.Unlock()
	return slices.Clone(m.kept)
}

// Lines is the lines kept, oldest first.
func (m *Memory) Lines() []string {
	m.mu.Lock()
	defer m.mu.Unlock()
	return slices.Clone(m.lines)
}

// Find is the records for which test holds.
func (m *Memory) Find(test func(*Record) bool) []*Record {
	return slices.DeleteFunc(m.Records(), func(r *Record) bool { return !test(r) })
}

// Last is the last record with this message, or nil.
func (m *Memory) Last(msg string) *Record {
	found := m.Find(func(r *Record) bool { return r.Msg == msg })
	if len(found) == 0 {
		return nil
	}
	return found[len(found)-1]
}

// Clear forgets everything.
func (m *Memory) Clear() {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.kept, m.lines = nil, nil
}

// Len is how many are kept.
func (m *Memory) Len() int {
	m.mu.Lock()
	defer m.mu.Unlock()
	return len(m.kept)
}

// Options describes the sink for export.
func (m *Memory) Options() map[string]any { return map[string]any{"size": cmp.Or(m.Size, 1000)} }
