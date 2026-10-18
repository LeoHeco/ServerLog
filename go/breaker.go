package serlog

import (
	"sync"
	"time"
)

// Health is whether a sink works: up, down, or probing after a failure.
type Health string

// The states of a sink.
const (
	Up      Health = "up"
	Down    Health = "down"
	Probing Health = "probing"
)

// Phase is which report of an incident this is.
type Phase string

// The reports of an incident.
const (
	Opened   Phase = "opened"
	Reminder Phase = "reminder"
	Resolved Phase = "resolved"
)

type tell int

const (
	report tell = iota
	silent
	folded
)

const longest = 300.0

// breaker is a slot's health, free of I/O so the shared vectors can drive it; times are seconds on a monotonic
// clock.
type breaker struct {
	mu       sync.Mutex
	health   Health
	since    time.Time
	skipped  int64
	err      string
	base     float64
	gap      float64
	next     float64
	told     float64
	hasTold  bool
	openedAt int64
}

func newBreaker(probe, now float64) *breaker {
	return &breaker{health: Up, since: time.Now(), base: probe, gap: probe, next: now}
}

func (b *breaker) up() bool {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.health == Up
}

func (b *breaker) admit(now float64) bool {
	b.mu.Lock()
	defer b.mu.Unlock()
	switch {
	case b.health == Up:
		return true
	case b.health == Down && now >= b.next:
		b.health = Probing
		return true
	}
	b.skipped++
	return false
}

func (b *breaker) resting(now float64) bool {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.health == Probing || b.health == Down && now < b.next
}

func (b *breaker) wait(now float64) float64 {
	b.mu.Lock()
	defer b.mu.Unlock()
	return max(b.next-now, 0)
}

func (b *breaker) passed() Phase {
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.health == Up {
		return ""
	}
	b.health, b.gap, b.since, b.err = Up, b.base, time.Now(), ""
	was := b.hasTold
	b.hasTold = false
	if was {
		return Resolved
	}
	return ""
}

func (b *breaker) failed(now float64, err string, remind float64, t tell) Phase {
	b.mu.Lock()
	defer b.mu.Unlock()
	b.err = err
	switch b.health {
	case Up:
		b.health, b.gap, b.since, b.openedAt = Down, b.base, time.Now(), b.skipped
		b.next = now + b.gap
		if t == silent {
			return ""
		}
		b.told, b.hasTold = now, true
		if t == report {
			return Opened
		}
	case Probing:
		b.health = Down
		b.gap = min(b.gap*2, longest)
		b.next = now + b.gap
		if t == report && b.hasTold && now-b.told >= remind {
			b.told = now
			return Reminder
		}
	}
	return ""
}

func (b *breaker) snapshot() (Health, time.Time, int64, int64, string) {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.health, b.since, b.skipped, b.skipped - b.openedAt, b.err
}
