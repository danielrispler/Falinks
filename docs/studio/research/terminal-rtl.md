# Hebrew (right-to-left) text in the studio's terminal pane

> **Answer:** The belief holds for the spike's WebGL renderer: xterm.js has no bidi, and every Hebrew word is drawn with its letters reversed. About 15 lines fix most of the display: a character joiner hands each Hebrew run to the browser, which draws it right to left in place. Typing and editing Hebrew stay awkward, because the cursor and the arrow keys follow the stored order, not the drawn order.
> **Evidence:** strong. It was tested in an offscreen WKWebView (the engine Tauri uses) with xterm.js 6.0.0 and addon-webgl 0.19.0, and against live Claude Code 2.1.311 output in a PTY. The Zed part was read from code, not seen on screen.
> **Options:** this supports (a-lite), the RTL-run joiner, inside R3.4. It supports a compose box (b) or a Ctrl+G editor bridge (c) as a later, separate ticket for comfortable typing. It rules out full renderer bidi (a-full) for now: it breaks Claude's layout (anthropics/claude-code#99046).
> **Read first:** [anthropics/claude-code#99046](https://github.com/anthropics/claude-code/issues/99046) (5 min) and [Terminal BiDi, basic modes](https://terminal-wg.pages.freedesktop.org/bidi/recommendation/basic-modes.html) (10 min).

## 1. What actually happens today

### Claude Code writes Hebrew in logical order and places every cell itself

I ran `claude` 2.1.311 in a PTY (100x30, `TERM=xterm-256color`). I typed `ab שלום עולם cd` without submitting and captured the bytes. Claude Code:

- writes each Hebrew letter in the order you type it, at the cursor (logical order). It does no reordering on macOS.
- moves the cursor over spaces with `CSI 1 C` instead of printing a space, and places some words with absolute column moves (`CSI 26 G`).
- turns on bracketed paste (`CSI ? 2004 h`) and synchronized output (`CSI ? 2026 h`).

I replayed the bytes into headless xterm.js. Row 6 of the buffer is `❯ ab שלום עולם cd`, stored in the order typed, with cursor column 17. So the buffer is correct. Only the drawing is wrong.

GitHub issues agree. [#89489](https://github.com/anthropics/claude-code/issues/89489) reports RTL drawn reversed "in all Claude Code TUI surfaces" (replies, tool previews, the live input) on terminals that do no bidi, including the VS Code terminal (xterm.js). [#99046](https://github.com/anthropics/claude-code/issues/99046) describes the opposite failure. On iTerm2 3.7, which runs bidi over each whole row, Claude's tables and mixed lines get scrambled, "because Claude Code writes RTL text in logical order and positions every cell itself, assuming left-to-right." It proposes that Claude Code switch bidi off where the terminal allows it (BDSM, `CSI 8 l`) and reorder RTL runs itself.

On Windows Claude Code already seems to reorder RTL itself. [#102870](https://github.com/anthropics/claude-code/issues/102870) says "On Windows, Claude Code orders RTL text itself, and each row takes its direction from its first strong letter". I did not verify this, but it is a risk for us (see the end of section 4a).

**So Claude Code does not reorder text on macOS today.** A renderer that reorders does not reverse it twice. But a renderer that reorders a whole row breaks Claude's layout, as #99046 shows. That is the key constraint.

### What xterm.js does, measured in WKWebView

Test page: xterm.js 6.0.0, Menlo 15px, the same text written to a DOM-rendered terminal and to a WebGL-rendered one. I measured the on-screen glyph order with `Range.getBoundingClientRect` (DOM) and from enlarged screenshots (WebGL).

| Renderer | `ab שלום עולם cd` reads as (right to left) | Why |
|---|---|---|
| WebGL (the spike's choice) | letters reversed: `םולש`, `םלוע` | each cell is drawn as a single glyph at its column ([addon-webgl draws per cell](https://github.com/xtermjs/xterm.js/tree/f447274f430fd22513f6adbf9862d19524471c04/addons/addon-webgl)). There is no bidi anywhere: the 6.0.0 bundles contain no `bidi` or `direction` code. |
| DOM | each word reads correctly, but **word order is wrong**: `עולם שלום` | the row is built from `<span>` runs ([DomRendererRowFactory.ts L196-L217](https://github.com/xtermjs/xterm.js/blob/f447274f430fd22513f6adbf9862d19524471c04/src/browser/renderer/dom/DomRendererRowFactory.ts#L196-L217)), and the browser applies bidi only inside each run |
| WebGL + RTL-run joiner (see 4a) | correct: `שלום עולם`; `❯ שורה אחת עם English באמצע` and a 51-letter Hebrew line also correct; box borders `│ … │` stay in place | the joined run is drawn as one string, and the browser's text engine orders it |

xterm.js maintainers say the same about the DOM renderer. Tyriar in [#701](https://github.com/xtermjs/xterm.js/issues/701) (2025-10-18): "it just happens to work in that case thanks to browser rendering. There are cases where this will fall over through (differing styles, line breaks)."

A `dir="rtl"` parent (the studio's cards are RTL) did not change the order inside the DOM terminal. The terminal container should still set `dir="ltr"` explicitly, because I did not test how the WebGL glyph canvas inherits direction.

## 2. Zed's terminal (what Daniel uses today)

Zed has no bidi code in its terminal. `grep -i 'bidi|rtl|hebrew'` over `crates/terminal` and `crates/terminal_view` at [`8a47b35`](https://github.com/zed-industries/zed/tree/8a47b3510959f5daba5c8b45bc3d04d68c5bd551) finds nothing. Its terminal grid comes from Zed's fork of `alacritty_terminal` ([Cargo.toml L538](https://github.com/zed-industries/zed/blob/8a47b3510959f5daba5c8b45bc3d04d68c5bd551/Cargo.toml#L538)). Alacritty itself has no bidi, and its maintainer says there is "extremely little interest in adding this" ([alacritty#663](https://github.com/alacritty/alacritty/issues/663)).

On macOS, though, Zed gets partial bidi by accident, the same way xterm.js's DOM renderer does:

1. Zed groups neighbouring cells of the same style on one row into a `BatchedTextRun` ([terminal_element.rs L95-L147](https://github.com/zed-industries/zed/blob/8a47b3510959f5daba5c8b45bc3d04d68c5bd551/crates/terminal_view/src/terminal_element.rs#L95-L147)). Plain spaces are "blank" and skipped ([L520](https://github.com/zed-industries/zed/blob/8a47b3510959f5daba5c8b45bc3d04d68c5bd551/crates/terminal_view/src/terminal_element.rs#L520), [`is_blank` L1880-L1898](https://github.com/zed-industries/zed/blob/8a47b3510959f5daba5c8b45bc3d04d68c5bd551/crates/terminal_view/src/terminal_element.rs#L1880-L1898)). A run continues only if the next cell is directly adjacent ([L550-L553](https://github.com/zed-industries/zed/blob/8a47b3510959f5daba5c8b45bc3d04d68c5bd551/crates/terminal_view/src/terminal_element.rs#L550-L553)), so **each word becomes its own run**.
2. Each run is shaped as one line of text with a forced cell width ([L149-L178](https://github.com/zed-industries/zed/blob/8a47b3510959f5daba5c8b45bc3d04d68c5bd551/crates/terminal_view/src/terminal_element.rs#L149-L178), `Some(dimensions.cell_width)` at L167).
3. On macOS, shaping goes through CoreText's `CTLine` ([gpui_apple text_system.rs L540-L590](https://github.com/zed-industries/zed/blob/8a47b3510959f5daba5c8b45bc3d04d68c5bd551/crates/gpui_apple/src/text_system.rs#L540-L590)). `CTLine` applies the Unicode bidi algorithm and returns glyphs in visual order. The code expects this: string indices can run backwards ([L622](https://github.com/zed-industries/zed/blob/8a47b3510959f5daba5c8b45bc3d04d68c5bd551/crates/gpui_apple/src/text_system.rs#L622)).
4. `apply_force_width_to_layout` snaps those glyphs, in visual order, onto consecutive cells ([line_layout.rs L753-L776](https://github.com/zed-industries/zed/blob/8a47b3510959f5daba5c8b45bc3d04d68c5bd551/crates/gpui/src/text_system/line_layout.rs#L753-L776)).

I checked step 3 with a 15-line Swift program that calls `CTLineCreateWithAttributedString`. For `ab שלום cd`, the Hebrew run comes back flagged right-to-left, glyphs `ם ו ל ש` at increasing x.

**Prediction for Zed on macOS:** each Hebrew word reads correctly, but a sentence's words appear in left-to-right order, as with the xterm.js DOM renderer. The cursor stays at the stored (logical) column. I could not see Zed's window to confirm this. Daniel can check in 10 seconds by running `echo "שלום עולם"` in Zed's terminal. If he sees `עולם שלום` when reading right to left, the prediction holds. If his Zed experience feels fine, that suggests word-level correctness is enough for him.

On Linux, Zed shapes text with cosmic-text instead, and users report RTL glyphs overlapping ([zed#63680](https://github.com/zed-industries/zed/issues/63680)). The general RTL issue is [zed#31102](https://github.com/zed-industries/zed/issues/31102).

## 3. What others do

- **Terminal BiDi recommendation** (Egmont Koblinger, v0.1, 2019, a draft): two modes. *Implicit* (the default): the terminal reorders, which suits simple utilities such as `cat`. *Explicit*: the app reorders and the terminal draws cells as written, which suits "sophisticated BiDi-aware apps". `CSI 8 l` / `CSI 8 h` (BDSM) switch between them. Apps with complex UIs that don't do BiDi can switch to explicit mode to keep their layout intact. [basic-modes](https://terminal-wg.pages.freedesktop.org/bidi/recommendation/basic-modes.html), [about](https://terminal-wg.pages.freedesktop.org/bidi/about.html).
- **GNOME Terminal / VTE:** implements that recommendation, level 1, from VTE 0.58 (GNOME 3.34, Sept 2019) ([implementations](https://terminal-wg.pages.freedesktop.org/bidi/implementations.html)).
- **Konsole:** applies bidi only within runs of identical attributes, so a colour change restarts it. **mlterm:** handles bidi, but rendering is inconsistent. **Apple Terminal:** does "some essential BiDi" with no way to turn it off, per the spec's prior-work page ([terminals](https://terminal-wg.pages.freedesktop.org/bidi/prior-work/terminals.html), 2019, from screenshots only).
- **iTerm2:** bidi has been behind an experimental setting since 3.5.11. A 2026-08 nightly turned it on by default ([v20260802-nightly](https://github.com/gnachman/iTerm2/releases/tag/v20260802-nightly)). This is the setting that broke Claude's layout in #99046. On 2026-10-01 iTerm2 added BDSM so apps can opt out ([commit 91411f5](https://github.com/gnachman/iTerm2/commit/91411f53619af602bd434ab355e674c1faa831e3)).
- **xterm.js:** [#701 "Support RTL languages"](https://github.com/xtermjs/xterm.js/issues/701), open since 2017, labelled help-wanted. Tyriar's 2019 attempt (PR #1899) joined RTL characters with the character-joiner API. Egmont asked him to hold off, warning that naive reordering makes BiDi-aware editing impossible. Maintainers' stance (2025): it "needs a native speaker... to research the topic and contribute this", and ultimately they'd want the terminal-wg layout. There is no bidi addon.
- **VS Code terminal:** it is xterm.js, so it has the same behaviour. Turning off `terminal.integrated.gpuAcceleration` gives the DOM renderer's word-level result (Tyriar, #701). The open umbrella issue is [vscode#11770](https://github.com/microsoft/vscode/issues/11770).
- **Warp:** no RTL in output or in its input editor. [warp#3589](https://github.com/warpdotdev/Warp/issues/3589) and [warp#10355](https://github.com/warpdotdev/Warp/issues/10355) are open.
- **Workarounds around Claude Code:** `termbidi` (a pipe filter, so `claude -p` only, not the interactive TUI) and the `rtl-terminal` plugin are linked from #89489 and #37183. I did not test either.

## 4. Approaches

### (a-lite) RTL-run character joiner on the WebGL renderer — recommended for R3.4

xterm.js has `registerCharacterJoiner(handler)`, a proposed API that the spike already enables with `allowProposedApi: true`. It is "only used by the webgl renderer" ([xterm.d.ts L1125-L1133](https://github.com/xtermjs/xterm.js/blob/f447274f430fd22513f6adbf9862d19524471c04/typings/xterm.d.ts#L1125-L1133)). The handler returns ranges of a row that should be drawn as one unit. Returning each run of Hebrew letters, plus the spaces and punctuation between them, makes the WebGL glyph canvas draw that run as one string. The browser then orders it right to left inside the same cells. The test used:

```js
const rtlRun = /[֐-׿]+(?:[ ,.?!'"-]+[֐-׿]+)*/g;
term.registerCharacterJoiner(t => [...t.matchAll(rtlRun)].map(m => [m.index, m.index + m[0].length]));
```

This is the in-place reordering of RTL runs that #99046 suggests Claude Code do itself. Borders, columns and English text stay where Claude put them. Only the Hebrew span is mirrored.

- **Cost:** about 15 lines plus a manual check. It fits R3.4.
- **Still broken:**
  - The cursor and arrow keys work on logical cells. While you type a Hebrew word, the cursor sits at its right edge, but the next letter appears at its left edge.
  - Mouse selection is mirrored inside a run.
  - Runs split where the style changes, because joiner ranges break on fg/bg changes ([CharacterJoinerService.ts L123](https://github.com/xtermjs/xterm.js/blob/f447274f430fd22513f6adbf9862d19524471c04/src/browser/services/CharacterJoinerService.ts#L123)). So a **bold** word in a Hebrew sentence sits in the wrong place: the same flaw the spec reports for Konsole.
  - Lines are left-aligned, not right-aligned.
  - Each wrapped row is reordered on its own. Reading top to bottom, each row right to left, still works.
  - Neutral characters at the edges of a run (a trailing `?`, digits) can sit on the wrong side.
- **Risks:**
  - Each distinct Hebrew run becomes an entry in the glyph cache, so long Hebrew output may cost performance. Untested.
  - If Claude Code someday reorders RTL itself on terminals that do no bidi (the second half of #99046, and apparently already done on Windows per #102870), the joiner would reverse the text again. Keep the joiner behind one flag.

### (a-full) Real bidi in the renderer (implicit mode over whole rows)

This means running the Unicode bidi algorithm over each row, as VTE, iTerm2 and Apple Terminal do. **Ruled out:** #99046 shows exactly this breaking Claude Code's tables, borders and mixed lines, because Claude places cells assuming no reordering. It is also weeks of work: a fork or patch of xterm.js renderers, cursor mapping, selection, and the BDSM mode. That is what xterm.js maintainers have avoided for 8 years.

### (b) Studio compose box (HTML RTL input, sent to the PTY)

This is a `<textarea dir="auto">` under the pane. On Enter it sends `ESC[200~ text ESC[201~` (bracketed paste) and then `\r`.

What I tested:

- A 3-line Hebrew bracketed paste lands as 3 lines in Claude's input, newlines kept.
- A 12-line paste becomes the chip `[Pasted text #1 +11 lines]`, with the hint "paste again to expand".

I did not test the `\r` submit after a paste.

- **Cost:** about 1 to 3 days for the box, its focus rules and key forwarding. It needs its own design decisions.
- **What breaks or needs care:**
  - Slash commands and their autocomplete appear only as keys reach the TUI. Forward keystrokes straight to the PTY when the box is empty or starts with `/`.
  - Image paste: Claude Code inserts an `[Image #N]` chip on `Ctrl+V` and reads the clipboard itself ([interactive mode docs](https://code.claude.com/docs/en/interactive-mode)). The box must forward `Ctrl+V` to the PTY. The chip then appears in Claude's input, not in the box.
  - History: Up in an empty box should go to the PTY, but the recalled prompt then shows in the TUI, not in the box.
  - Permission prompts, menus and `Esc` must keep going to the TUI.
  - Claude's own drawing is untouched, which is good.
- **Output:** the box does nothing for Claude's replies in the pane (see section 5).

### (c) Ctrl+G editor bridge

Claude Code opens the current prompt in the default text editor on `Ctrl+G` or `Ctrl+X Ctrl+E`, and reads it back when the editor exits (interactive mode docs). The PTY spike showed the hint "ctrl+g to edit in VS Code". If the studio sets `VISUAL`/`EDITOR` for the Claude process to a small helper, the helper could open an RTL HTML editor in the studio and exit on submit. That keeps Claude's own draft, history recall (Up, then Ctrl+G), and chips.

- **Cost:** a helper binary, IPC to the studio, and a modal. Similar to (b), with fewer focus rules.
- **Unverified:** which variable Claude reads first (`VISUAL` or `EDITOR`), and whether it passes chips through as text.

### (d) Other options found

- DOM renderer instead of WebGL: gives word-level correctness for free, but word order is wrong and it is slower. (a-lite) on WebGL is better.
- Answer BDSM: make xterm.js reply to `CSI 8 $ p` so that a future Claude Code that reorders RTL itself can detect us. This only matters if Anthropic ships #99046. Watch it, but don't build it.
- A headless stream-json client renders HTML natively, but it was set aside. Nothing here forces it back.

## 5. With a compose box, is Claude's output still reversed? Does it matter?

Yes. A compose box fixes only typing. Claude's Hebrew replies, which are the usual answer to a Hebrew prompt, are drawn in the pane by Claude's TUI. With plain WebGL every Hebrew word in them is reversed. The studio's cards are separate summaries and do not replace reading Claude's replies. So output matters at least as much as input. That is why the joiner (a-lite) comes first: it fixes the reading of replies, tool previews and the live input line at once. The compose box (b or c) only improves editing comfort, and Daniel can judge after using (a-lite).

## 6. Recommendation

Put (a-lite) into **R3.4**: the joiner plus `dir="ltr"` on the terminal container. It is renderer setup in the pane being built, about 15 lines, and it decides whether the pane is readable at all.

Open a **separate ticket** for (b) or (c), decided after a few days of use. It is a new input surface with its own design decisions (slash autocomplete, image chips, history, focus), and it may turn out to be unnecessary.

## Sources checked

Checked against the source in this session:

- Zed at `8a47b35`: `terminal_element.rs` (batching, `is_blank`, `shape_line` with forced width), `gpui_apple/src/text_system.rs` (`CTLine`), `gpui/src/text_system/line_layout.rs` (`apply_force_width_to_layout`), `Cargo.toml` (alacritty fork). I also grepped for bidi and found none. CoreText's bidi reordering was confirmed by running a Swift probe.
- xterm.js 6.0.0 (tag `f447274`): `DomRendererRowFactory.ts` span merging, `CharacterJoinerService.ts` splitting on fg/bg, `xterm.d.ts` "joiners are only used by the webgl renderer", and no bidi code in the shipped bundles. Behaviour was rendered in an offscreen WKWebView: DOM, WebGL, WebGL with joiner, and DOM in an RTL parent.
- Claude Code 2.1.311, live in a PTY: logical-order writes, `CSI C` for spaces, bracketed paste results (inline at 3 lines, a chip at 12).
- Issues read: anthropics/claude-code #99046, #89489, #102870, #37183; xtermjs/xterm.js #701 (maintainer comments); alacritty #663; zed #63680; warp #10355; vscode #327965 (closed as a duplicate).
- iTerm2 commit 91411f5 (BDSM), via the GitHub API. The iTerm2 nightly v20260802 notes and the terminal-wg pages (basic modes, prior-work terminals, implementations, about) were fetched.
- Claude Code docs, interactive mode: Ctrl+G, Ctrl+V image chip.

Not verified, or resting on others' reports:

- The Zed on-screen result (a prediction from code).
- Konsole, mlterm and Apple Terminal behaviour (the spec's 2019 notes only).
- VTE details beyond "level 1 in 0.58".
- The claim in #102870 that Claude Code reorders text on Windows.
- Submitting with `\r` after a bracketed paste, and image paste through a compose box.
- `VISUAL` vs `EDITOR` precedence for Ctrl+G.
- Joiner performance on long Hebrew output, and how the WebGL canvas inherits direction inside an RTL page.
- Hebrew with niqqud. I tested no Arabic.
