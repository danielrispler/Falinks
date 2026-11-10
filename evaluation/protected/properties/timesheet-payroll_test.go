package timesheet_test

// Differential properties P1-P9, one per ticket (#552; evaluation/code-quality.json). Each
// property prints one line per seeded input; `quality` compares the lines with the frozen
// reference's. Only the API the oracle imports is used. The requirements fix which sentinels
// an error satisfies, the `line N: ` prefixes, the duplicate and missing-rate messages and the
// `entries: ` and `rates: ` prefixes, so those are printed; other error text is not.
// WeekStart follows the corrected T2 (the last Monday strictly before the date), as the oracle
// does.

import (
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"strconv"
	"strings"
	"testing"
	"time"

	"example.com/timesheet/clock"
	"example.com/timesheet/entry"
	"example.com/timesheet/money"
	"example.com/timesheet/payroll"
	"example.com/timesheet/report"
)

const seed, inputs = 228, 500

// rng is splitmix64.
type rng struct{ s uint64 }

func (r *rng) next() uint64 {
	r.s += 0x9e3779b97f4a7c15
	z := r.s
	z = (z ^ (z >> 30)) * 0xbf58476d1ce4e5b9
	z = (z ^ (z >> 27)) * 0x94d049bb133111eb
	return z ^ (z >> 31)
}

func (r *rng) n(k int) int { return int(r.next() % uint64(k)) }

// between is an integer from lo to hi, both included.
func (r *rng) between(lo, hi int) int { return lo + r.n(hi-lo+1) }

func (r *rng) pick(pool []string) string { return pool[r.n(len(pool))] }

func emit(id string, i int, value func() any) {
	line := func() (line string) {
		defer func() {
			if recover() != nil {
				line = "panic"
			}
		}()
		b, err := json.Marshal(value())
		if err != nil {
			return strconv.Quote(err.Error())
		}
		return string(b)
	}()
	fmt.Printf("falinks-property %s %d %s\n", id, i, line)
}

var sentinels = []struct {
	name string
	err  error
}{
	{"ErrInvalidDuration", clock.ErrInvalidDuration}, {"ErrInvalidDate", clock.ErrInvalidDate},
	{"ErrInvalidAmount", money.ErrInvalidAmount}, {"ErrInvalidEntry", entry.ErrInvalidEntry},
	{"ErrDuplicateEntry", entry.ErrDuplicateEntry}, {"ErrInvalidRate", payroll.ErrInvalidRate},
	{"ErrMissingRate", report.ErrMissingRate},
}

// kinds names every sentinel err satisfies, or is nil for a nil error.
func kinds(err error) any {
	if err == nil {
		return nil
	}
	out := []string{}
	for _, s := range sentinels {
		if errors.Is(err, s.err) {
			out = append(out, s.name)
		}
	}
	return out
}

// head is an error's message up to its `n`th ": ", or all of it.
func head(err error, n int) string {
	if err == nil {
		return ""
	}
	parts := strings.SplitN(err.Error(), ": ", n+1)
	return strings.Join(parts[:min(n, len(parts))], ": ")
}

func show(t time.Time) string { return t.Format(time.RFC3339Nano) + " " + t.Location().String() }

func digits(r *rng, n int) string {
	b := make([]byte, n)
	for i := range b {
		b[i] = byte('0' + r.n(10))
	}
	return string(b)
}

func chars(r *rng, set string, n int) string {
	b := make([]byte, n)
	for i := range b {
		b[i] = set[r.n(len(set))]
	}
	return string(b)
}

// day is midnight UTC on a date mostly near 2026-11, sometimes anywhere in years 1-9999.
func day(r *rng) time.Time {
	if r.n(5) == 0 {
		return time.Date(r.between(1, 9999), 1, 1, 0, 0, 0, 0, time.UTC).AddDate(0, 0, r.n(365))
	}
	return time.Date(2026, 11, 1, 0, 0, 0, 0, time.UTC).AddDate(0, 0, r.between(-60, 60))
}

// moment is day(r), at a random time of day in a random fixed zone half the time.
func moment(r *rng) time.Time {
	d := day(r)
	if r.n(2) == 0 {
		return d
	}
	zone := time.FixedZone("z", r.between(-14*4, 14*4)*900)
	return time.Date(d.Year(), d.Month(), d.Day(), r.n(24), r.n(60), 0, 0, zone)
}

var durationEdges = []string{"7:30", "07:30", "0:05", "00:00", "99:59", "100:00", "7:3", "7:60",
	"730", "", " 7:30", "7:30 ", "+7:30", "-7:30", "7::30", ":30", "7:", "24:00", "24:01", "0:00"}

// p1 checks ParseDuration on strings near H:MM, and String on any value but the minimum.
func p1(r *rng) any {
	s := digits(r, r.n(4)) + ":" + digits(r, r.n(4))
	switch r.n(4) {
	case 0:
		s = r.pick(durationEdges)
	case 1:
		s = chars(r, "0123456789: +-\t", r.n(8))
	}
	m, err := clock.ParseDuration(s)
	v := clock.Minutes(r.between(-20000, 20000))
	if r.n(5) == 0 {
		v = clock.Minutes(int64(r.next()>>1) - math.MaxInt64/2)
	}
	return []any{s, int(m), kinds(err), int(v), v.String()}
}

var dateEdges = []string{"2028-02-29", "2026-02-29", "1900-02-29", "2000-02-29", "2026-13-01",
	"0000-01-01", "0001-01-01", "9999-12-31", "2026-1-05", "2026/01/05", " 2026-01-05",
	"2026-01-05 ", "", "2026-04-31", "+026-01-05", "2026-01-05T00:00:00Z"}

// p2 checks ParseDate on strings near YYYY-MM-DD, and WeekStart (as corrected) on any moment.
func p2(r *rng) any {
	s := fmt.Sprintf("%04d-%02d-%02d", r.between(0, 10000), r.between(0, 13), r.between(0, 32))
	switch r.n(5) {
	case 0:
		s = day(r).Format("2006-01-02")
	case 1:
		s = r.pick(dateEdges)
	case 2:
		s = chars(r, "0123456789-/ ", 10)
	}
	parsed, err := clock.ParseDate(s)
	t := moment(r)
	return []any{s, show(parsed), kinds(err), show(t), show(clock.WeekStart(t))}
}

var amountEdges = []string{"25", "25.50", "0.99", "007.00", "", ".50", "25.", "25.5", "25.505",
	"-1.00", "+1.00", "1,000.00", "1e3", "1234567890", "999999999.99", " 25", "25 ", "0.00", "0"}

// p3 checks ParseAmount on strings near the syntax, and String over int64.
func p3(r *rng) any {
	s := digits(r, r.between(0, 11))
	if r.n(2) == 0 {
		s += "." + digits(r, r.n(4))
	}
	switch r.n(4) {
	case 0:
		s = r.pick(amountEdges)
	case 1:
		s = chars(r, "0123456789.,-+ e", r.n(9))
	}
	parsed, err := money.ParseAmount(s)
	c := money.Cents(r.next())
	switch r.n(4) {
	case 0:
		c = money.Cents([]int64{math.MinInt64, math.MaxInt64, 0, -1, 99, -100, 100000, -99999}[r.n(8)])
	case 1:
		c = money.Cents(r.between(-10000000, 10000000))
	}
	return []any{s, int64(parsed), kinds(err), int64(c), c.String()}
}

var ids = []string{"alice", "bob", "j-smith2", "a", "z9-", "Alice", "2pac", "j_smith", "-bob", "",
	"al ice", "alicé", strings.Repeat("a", 32), strings.Repeat("b", 33), "x" + strings.Repeat("-", 31)}

// p4 checks ValidEmployee, and ParseEntry on lines near the format.
func p4(r *rng) any {
	id := r.pick(ids)
	if r.n(3) == 0 {
		id = chars(r, "abcxyzAZ09-_ ", r.between(0, 34))
	}
	date := day(r).Format("2006-01-02")
	if r.n(5) == 0 {
		date = r.pick(dateEdges)
	}
	worked := fmt.Sprintf("%d:%02d", r.between(0, 25), r.n(60))
	if r.n(5) == 0 {
		worked = r.pick(durationEdges)
	}
	fields := []string{date, r.pick(ids), worked}
	pad := []string{"", "", "", " ", "\t", " \t "}
	for i := range fields {
		fields[i] = r.pick(pad) + fields[i] + r.pick(pad)
	}
	if r.n(8) == 0 {
		fields = fields[:r.n(3)]
	} else if r.n(8) == 0 {
		fields = append(fields, r.pick(pad))
	}
	line := strings.Join(fields, ",")
	e, err := entry.ParseEntry(line)
	return []any{id, entry.ValidEmployee(id), line, show(e.Date), e.Employee, int(e.Worked), kinds(err)}
}

// sheetLine is a timesheet line: valid nine times in ten, for a few employees and dates so
// that some repeat.
func sheetLine(r *rng, employees []string) string {
	date := time.Date(2026, 11, 1, 0, 0, 0, 0, time.UTC).AddDate(0, 0, r.n(21)).Format("2006-01-02")
	line := fmt.Sprintf("%s,%s,%d:%02d", date, r.pick(employees), r.between(0, 14), r.n(60))
	if line[len(line)-4:] == "0:00" && line[len(line)-5] == ',' {
		line = line[:len(line)-4] + "0:30"
	}
	if r.n(10) == 0 {
		line = r.pick([]string{"2026-11-31,alice,1:00", "2026-11-02,Bob,1:00", "2026-11-02,bob,1:0",
			"2026-11-02,bob", "2026-11-02,bob,25:00", "2026-11-02,bob,0:00", "bad", "2026-11-02,bob,1:00,"})
	}
	return line
}

// sheet is a file of 0-7 lines: entries, blank, space-only and comment lines, with \n or \r\n,
// and a final newline or not.
func sheet(r *rng, employees []string, lines int) string {
	var b strings.Builder
	for i := 0; i < lines; i++ {
		line := sheetLine(r, employees)
		switch r.n(8) {
		case 0:
			line = r.pick([]string{"", " ", "\t ", "# note", "  # x", "\t#", " x # not a comment"})
		case 1:
			line = r.pick([]string{" ", "\t"}) + line
		}
		b.WriteString(line)
		if i < lines-1 || r.n(2) == 0 {
			b.WriteString(r.pick([]string{"\n", "\n", "\r\n"}))
		}
	}
	return b.String()
}

// p5 checks ParseEntries on short files, with repeats and an occasional bad line.
func p5(r *rng) any {
	file := sheet(r, []string{"alice", "bob", "carol"}, r.n(8))
	entries, err := entry.ParseEntries(strings.NewReader(file))
	out := [][]any{}
	for _, e := range entries {
		out = append(out, []any{show(e.Date), e.Employee, int(e.Worked)})
	}
	message := head(err, 1)
	if errors.Is(err, entry.ErrDuplicateEntry) {
		message = err.Error()
	}
	return []any{file, entries == nil, out, kinds(err), message}
}

// p6 checks Summarize on 0-8 entries at any time of day and zone, with weekly totals around 40:00.
func p6(r *rng) any {
	var in []entry.Entry
	if r.n(10) > 0 {
		in = make([]entry.Entry, r.n(9))
	}
	for i := range in {
		in[i] = entry.Entry{Date: moment(r), Employee: r.pick([]string{"alice", "bob", "b", "alice-2"}),
			Worked: clock.Minutes(r.between(0, 1440))}
		if r.n(4) == 0 && i > 0 {
			in[i].Date = in[i-1].Date.AddDate(0, 0, r.between(-1, 1))
		}
		if r.n(6) == 0 {
			in[i].Worked = clock.Minutes(r.between(2000, 2800))
		}
	}
	before := fmt.Sprint(in)
	got := payroll.Summarize(in)
	out := [][]any{}
	for _, s := range got {
		out = append(out, []any{s.Employee, show(s.WeekStart), int(s.Regular), int(s.Overtime)})
	}
	return []any{fmt.Sprint(in), got == nil, out, fmt.Sprint(in) == before}
}

// p7 checks WeekPay for rates up to 99,999,999,999 and up to 10,080 minutes, some negative.
func p7(r *rng) any {
	rate := money.Cents(r.between(0, 99999999999))
	switch r.n(4) {
	case 0:
		rate = money.Cents(r.between(0, 10))
	case 1:
		rate = money.Cents(r.between(0, 10000))
	}
	regular := clock.Minutes(r.between(0, 2400))
	overtime := clock.Minutes(r.between(0, 10080-int(regular)))
	if r.n(3) == 0 {
		overtime = 0
	}
	if r.n(12) == 0 {
		regular = -regular - 1
	} else if r.n(12) == 0 {
		overtime = -overtime - 1
	} else if r.n(12) == 0 {
		rate = -rate - 1
	}
	s := payroll.WeekSummary{Employee: "alice", WeekStart: day(r), Regular: regular, Overtime: overtime}
	return []any{int64(rate), int(regular), int(overtime), int64(payroll.WeekPay(rate, s))}
}

// p8 checks ParseRates on files of 0-5 lines. A line never has both a bad ID and a malformed
// amount, the one case whose sentinels the requirement leaves open.
func p8(r *rng) any {
	var b strings.Builder
	lines := r.n(6)
	for i := 0; i < lines; i++ {
		id := r.pick([]string{"alice", "bob", "carol", "alice", "bob"})
		amount := r.pick([]string{"25.00", "19.99", "7", "0.01", "999999999.99", "100"})
		switch r.n(10) {
		case 0:
			id = r.pick([]string{"", "Alice", "2pac", "j_smith", strings.Repeat("a", 33)})
		case 1:
			amount = r.pick([]string{"", "2=5", "25.5", "-1.00", "1,000", "1234567890", ".50"})
		case 2:
			amount = r.pick([]string{"0.00", "0", "000.00"})
		}
		pad := []string{"", "", " ", "\t", " \t"}
		line := r.pick(pad) + id + r.pick(pad) + "=" + r.pick(pad) + amount + r.pick(pad)
		switch r.n(10) {
		case 0:
			line = r.pick([]string{"", "  ", "# rates", " # x", "alice 25.00", "alice"})
		}
		b.WriteString(line)
		if i < lines-1 || r.n(2) == 0 {
			b.WriteString(r.pick([]string{"\n", "\n", "\r\n"}))
		}
	}
	rates, err := payroll.ParseRates(strings.NewReader(b.String()))
	out := map[string]int64{}
	for k, v := range rates {
		out[k] = int64(v)
	}
	return []any{b.String(), rates == nil, out, kinds(err), head(err, 1)}
}

// p9 checks Weekly on short timesheets and rate files, mostly valid, some IDs long.
func p9(r *rng) any {
	employees := []string{"alice", "bob", r.pick([]string{"carol", "a-much-longer-employee-id", "dan"})}
	entries := sheet(r, employees, r.n(9))
	var rates strings.Builder
	for _, e := range employees {
		if r.n(8) > 0 {
			fmt.Fprintf(&rates, "%s=%d.%02d\n", e, r.between(0, 200), r.n(100))
		}
	}
	if r.n(10) == 0 {
		rates.WriteString(r.pick([]string{"alice=0.00\n", "zed\n", "bob=1.5\n"}))
	}
	text, err := report.Weekly(strings.NewReader(entries), strings.NewReader(rates.String()))
	message := head(err, 2)
	if errors.Is(err, entry.ErrDuplicateEntry) || errors.Is(err, report.ErrMissingRate) {
		message = err.Error()
	}
	return []any{entries, rates.String(), text, kinds(err), message}
}

func TestFalinksProperties(t *testing.T) {
	for _, p := range []struct {
		id    string
		check func(*rng) any
	}{{"P1", p1}, {"P2", p2}, {"P3", p3}, {"P4", p4}, {"P5", p5}, {"P6", p6}, {"P7", p7}, {"P8", p8}, {"P9", p9}} {
		r := &rng{seed}
		for i := 0; i < inputs; i++ {
			emit(p.id, i, func() any { return p.check(r) })
		}
	}
}
