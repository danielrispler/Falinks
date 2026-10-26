// Throwaway: the guess-and-card loop shared by the three mockups, copied from the v0 panel (#315).
// One fake step: the agent edited src/auth.rs and is about to run the tests. All data is made up.
const STEP = {
  context: 'Bash(cargo test auth::)   after   Update(src/auth.rs)',
  question: 'הסוכן שינה את בדיקת התפוגה של הטוקן. האם כל בדיקות auth יעברו?',
  // #271: name the goal first, one idea, at most about four new things, labels on the code itself
  title: 'המטרה: טוקן נדחה כבר ברגע התפוגה. בדיקה אחת הניחה אחרת.',
  code: [
    { line: '// src/auth.rs', label: '' },
    // \u2066…\u2069 keeps < and <= left to right; inside Hebrew text they would show mirrored
    { line: 'if token.expires_at <= now {', label: 'היה \u2066<\u2069, עכשיו \u2066<=\u2069' },
    { line: '    return Err(AuthError::Expired);', label: 'קורה עכשיו גם ברגע התפוגה עצמו' },
    { line: '}', label: '' },
    { line: '// cargo test: 11 passed, 1 failed', label: '' },
    { line: 'accepts_token_at_expiry ... FAILED', label: 'הבדיקה ציפתה להתנהגות הישנה' },
  ],
  unchecked: 'לא נבדק: מה קורה כשהשעון של השרת לא מסונכרן.',
};

const span = (text, className = '') => Object.assign(document.createElement('span'), { textContent: text, className });

// Runs ask, wait, card, idle inside root. Each mockup supplies the markup through data-part names.
function loop(root, onCard) {
  const part = name => root.querySelector(`[data-part="${name}"]`);
  const show = state => ['ask', 'wait', 'card', 'idle'].forEach(s => { if (part(s)) part(s).hidden = s !== state; });
  part('ctx').textContent = STEP.context;
  part('q').textContent = STEP.question;
  const send = () => {
    const guess = part('guess').value.trim();
    if (!guess) return;
    part('guess').value = '';
    show('wait');
    setTimeout(() => { // fake: the step finishes
      part('title').textContent = STEP.title;
      part('code')?.replaceChildren(...STEP.code.map(({ line, label }) => {
        const row = document.createElement('div');
        row.className = 'l';
        row.append(span(line), span(label && '← ' + label, 'lab'));
        return row;
      }));
      part('yours').textContent = 'ניחשת: ' + guess;
      part('unchecked').textContent = STEP.unchecked;
      show('card');
      onCard?.(guess);
    }, 1500);
  };
  part('send').onclick = send;
  part('guess').onkeydown = e => { if (e.key === 'Enter' && e.metaKey) send(); };
  part('done').onclick = () => show('idle');
  if (part('again')) part('again').onclick = () => show('ask');
  show('ask');
}
