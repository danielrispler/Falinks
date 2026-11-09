package lending_test

// Differential properties L1-L9, one per ticket (#509; evaluation/code-quality.json). Each
// property prints one line per seeded input; `quality` compares the lines with the frozen
// reference's. Only the API the oracle imports is used. The requirements fix every sentinel
// and T8's error messages, so those are printed; nothing is printed for the behaviour T2
// leaves unspecified (AddDays, Weekday and DaysBetween on invalid dates). DaysBetween follows
// the corrected T2 (both ends counted), as the oracle does.

import (
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"strconv"
	"strings"
	"testing"

	"example.com/lending/calendar"
	"example.com/lending/item"
	"example.com/lending/loan"
	"example.com/lending/money"
	"example.com/lending/report"
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

// sentinel names which of the requirements' errors err is, or "other".
func sentinel(err error) any {
	for name, e := range map[string]error{
		"ErrInvalidAmount": money.ErrInvalidAmount, "ErrInvalidDate": calendar.ErrInvalidDate,
		"ErrUnknownKind": item.ErrUnknownKind, "ErrInvalidKind": loan.ErrInvalidKind,
		"ErrInvalidRenewals": loan.ErrInvalidRenewals, "ErrOverdue": loan.ErrOverdue,
		"ErrRenewalLimit": loan.ErrRenewalLimit, "ErrInvalidCategory": loan.ErrInvalidCategory,
		"ErrTotalLimit": loan.ErrTotalLimit, "ErrKindLimit": loan.ErrKindLimit,
	} {
		if err == e {
			return name
		}
	}
	if err == nil {
		return nil
	}
	return "other"
}

func digits(r *rng, n int) string {
	b := make([]byte, n)
	for i := range b {
		b[i] = byte('0' + r.n(10))
	}
	return string(b)
}

// day is a valid date in years 1900 to 2999, mostly near 2026.
func day(r *rng) calendar.Date {
	if r.n(4) == 0 {
		return calendar.Date{Year: r.between(1900, 2999), Month: 1, Day: 1}.AddDays(r.n(366))
	}
	return calendar.Date{Year: 2026, Month: 1, Day: 1}.AddDays(r.between(-400, 800))
}

// kind is a valid kind five times in six.
func kind(r *rng) item.Kind {
	if r.n(6) > 0 {
		return item.Kind(r.between(1, 3))
	}
	return item.Kind([]int{0, 4, -1, 9}[r.n(4)])
}

var amountEdges = []string{"92233720368547758.07", "92233720368547758.08", "-92233720368547758.08",
	"-92233720368547758.09", "-0", "007.05", "12.", ".5", "+1", " 1", "1,000", "", "-"}

// l1 checks Format over int64 and Parse on amounts near and outside the syntax.
func l1(r *rng) any {
	c := money.Cents(r.next())
	switch r.n(4) {
	case 0:
		c = money.Cents([]int64{math.MinInt64, math.MaxInt64, 0, -1, 99, -100}[r.n(6)])
	case 1:
		c = money.Cents(r.between(-100000, 100000))
	}
	s := r.pick(amountEdges)
	switch r.n(3) {
	case 0:
		s = r.pick([]string{"", "-"}) + digits(r, 1+r.n(20))
		if r.n(2) == 0 {
			s += "." + digits(r, r.n(4))
		}
	case 1:
		const chars = "0123456789-.+ ,"
		b := make([]byte, r.n(8))
		for i := range b {
			b[i] = chars[r.n(len(chars))]
		}
		s = string(b)
	}
	parsed, err := money.Parse(s)
	back, backErr := money.Parse(money.Format(c))
	return []any{money.Format(c), s, int64(parsed), sentinel(err), int64(back), sentinel(backErr)}
}

// l2 checks Parse on near-dates, and String, AddDays, Weekday and DaysBetween on valid ones.
func l2(r *rng) any {
	s := fmt.Sprintf("%04d-%02d-%02d", r.between(1890, 3010), r.between(0, 13), r.between(0, 32))
	switch r.n(5) {
	case 0:
		s = day(r).String()
	case 1:
		s = r.pick([]string{"2024-02-29", "2026-02-29", "1900-02-29", "2000-02-29", "2026-1-05",
			"2026-01-05 ", " 2026-01-05", "2026/01/05", "+026-01-05", "", "1899-12-31", "3000-01-01"})
	}
	parsed, err := calendar.Parse(s)
	d, e := day(r), day(r)
	n := r.between(-2000, 2000)
	return []any{s, parsed, sentinel(err), d.String(), d.AddDays(n), d.AddDays(-n).AddDays(n) == d,
		int(d.Weekday()), calendar.DaysBetween(d, e), calendar.DaysBetween(e, d)}
}

// l3 checks ParseKind on spellings near the three names, and String on any Kind.
func l3(r *rng) any {
	name := []byte(r.pick([]string{"book", "dvd", "magazine", "books", "cd", "", "booK", "dv d"}))
	for i := range name {
		if r.n(2) == 0 && 'a' <= name[i] && name[i] <= 'z' {
			name[i] -= 'a' - 'A'
		}
	}
	pad := []string{"", "", " ", "\t", " \t", "\n", "\r"}
	s := r.pick(pad) + string(name) + r.pick(pad)
	k, err := item.ParseKind(s)
	other := item.Kind(r.between(-2, 5))
	return []any{s, int(k), sentinel(err), other.String()}
}

// l4 checks DueDate for any kind and valid checkout date.
func l4(r *rng) any {
	k, checkout := kind(r), day(r)
	due, err := loan.DueDate(k, checkout)
	return []any{int(k), checkout, due, sentinel(err)}
}

// l5 checks LateFee for returns from 10 days early to 90 days late.
func l5(r *rng) any {
	k, due := kind(r), day(r)
	returned := due.AddDays(r.between(-10, 90))
	fee, err := loan.LateFee(k, due, returned)
	return []any{int(k), due, returned, int64(fee), sentinel(err)}
}

// l6 checks Renew around the due date with renewal counts from -1 to 3.
func l6(r *rng) any {
	k, due := kind(r), day(r)
	today := due.AddDays(r.between(-6, 4))
	renewals := r.between(-1, 3)
	next, err := loan.Renew(k, due, today, renewals)
	return []any{int(k), due, today, renewals, next, sentinel(err)}
}

// l7 checks CanBorrow for any category, holdings of 0-12 items and next kind.
func l7(r *rng) any {
	cat := loan.Category(r.between(1, 2))
	if r.n(8) == 0 {
		cat = loan.Category([]int{0, 3, -1}[r.n(3)])
	}
	var current []item.Kind
	if r.n(10) > 0 {
		current = make([]item.Kind, r.n(13))
		for i := range current {
			current[i] = item.Kind(r.between(1, 3))
			if r.n(25) == 0 {
				current[i] = kind(r)
			}
		}
	}
	next := item.Kind(r.between(1, 3))
	if r.n(10) == 0 {
		next = kind(r)
	}
	err := loan.CanBorrow(cat, current, next)
	held := make([]int, len(current))
	for i, k := range current {
		held[i] = int(k)
	}
	return []any{int(cat), held, int(next), sentinel(err)}
}

// l8 checks ParseLoan on lines near the format, its values and its error messages.
func l8(r *rng) any {
	ids := []string{"BK-0042", "DVD-0001", "MAG-7", "A", "---", "12345678901234567890",
		"123456789012345678901", "", "bk-1", "BK_1", "BK 1", "BK-É"}
	kinds := []string{"book", "DVD", "Magazine", "books", "", "cd"}
	date := func() string {
		if r.n(6) == 0 {
			return r.pick([]string{"2026-02-29", "1899-12-31", "2026-1-05", "x", "", "3000-01-01"})
		}
		return day(r).String()
	}
	checkout := date()
	returned := ""
	switch r.n(4) {
	case 0:
		returned = date()
	case 1, 2:
		if d, err := calendar.Parse(checkout); err == nil {
			returned = d.AddDays(r.between(-3, 30)).String()
		}
	}
	fields := []string{r.pick(ids), r.pick(kinds), checkout, returned}
	pad := []string{"", "", "", " ", "\t", " \t "}
	for i := range fields {
		fields[i] = r.pick(pad) + fields[i] + r.pick(pad)
	}
	if r.n(8) == 0 {
		fields = fields[:r.n(4)]
	} else if r.n(8) == 0 {
		fields = append(fields, r.pick(pad))
	}
	line := strings.Join(fields, ",") + r.pick([]string{"", "", "\n", "\r\n", "\r", "\n\n", "\r\r\n"})
	l, err := report.ParseLoan(line)
	message := ""
	if err != nil {
		message = err.Error()
	}
	return []any{line, l, err != nil && errors.Is(err, report.ErrMalformedLine), message}
}

// l9 checks the overdue report on 0-6 loans, some returned, with repeated IDs.
func l9(r *rng) any {
	today := day(r)
	loans := make([]report.Loan, r.n(7))
	for i := range loans {
		k := item.Kind(r.between(1, 3))
		if r.n(15) == 0 {
			k = kind(r)
		}
		loans[i] = report.Loan{ItemID: r.pick([]string{"A", "B", "BK-1", "B-2", "MAG-7"}), Kind: k,
			Checkout: today.AddDays(-r.between(0, 60))}
		if r.n(2) == 0 && i > 0 {
			loans[i].Checkout = loans[i-1].Checkout
		}
		if r.n(4) == 0 {
			loans[i].Returned, loans[i].IsReturned = loans[i].Checkout.AddDays(r.n(30)), true
		}
	}
	text, err := report.OverdueReport(loans, today)
	return []any{loans, today, text, err != nil && errors.Is(err, loan.ErrInvalidKind)}
}

func TestFalinksProperties(t *testing.T) {
	for _, p := range []struct {
		id    string
		check func(*rng) any
	}{{"L1", l1}, {"L2", l2}, {"L3", l3}, {"L4", l4}, {"L5", l5}, {"L6", l6}, {"L7", l7}, {"L8", l8}, {"L9", l9}} {
		r := &rng{seed}
		for i := 0; i < inputs; i++ {
			emit(p.id, i, func() any { return p.check(r) })
		}
	}
}
