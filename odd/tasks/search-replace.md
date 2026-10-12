# Feature: Search & Replace (editormini)

**Goal:** VS Code-like find/replace: active-document search dialog with match navigation and highlighting; active-document replace; cross-document search and replace over all open tabs. Design forward-compatible with future options (case-sensitive toggle, regex).

**Owner decisions:** Ctrl+F active-doc find with dialog; Ctrl+Shift+F find across all open docs; Ctrl+H active-doc replace; Ctrl+Shift+H replace across all open docs. Case-sensitive/regex options are FUTURE (design the options struct now, do not implement toggles yet). All dialogs i18n EN/ES. Every action needs a chord AND a documented F-key fallback (mintty swallows some chords — precedent F2).

## Tasks

- [x] **1. Find core + Ctrl+F active doc** — SearchState dialog (extend/parallel to PromptKind modals): input field, live match count "current/total", Enter = next match (wrap), Shift+Enter = prev match (wrap), Esc = close (restore cursor pre-search position if no navigation happened), match highlighting in viewport (current match distinct from other matches), empty query = no-op navigation, no-match visible message. SearchOptions struct (case_sensitive: bool default false — i.e., case-insensitive default; is_regex: bool default false) present in the API but no UI toggles. Search is over the document text (line-based buffer → joined with the document's line ending semantics for cross-line matches OUT OF SCOPE: single-line matches only for this feature). **Verify:** tests + USER. **Commit:** `d8f52cc` (11 new tests, 125 total, 0 warnings; dialog composes with gutter; theme fields search_match_bg/search_current_match_bg Dark+ + Light+ stubs). **USER PENDING: probar Ctrl+F.** Native review (lineage `review-86d209662de374fc`) found 3 REAL severe issues (Shift+chars dropped in query; dialog covered by text render; match lines lost syntax spans) + 1 false positive (Backspace panic). Correction DECLINED: fix diff (275 lines) exceeded the 200-line correction slot cap; fixes applied as ordinary work unit `e49cf4e` (128 tests, 0 warnings). Lineage left open in correction_required.
- [x] **2. Ctrl+H replace active doc** — replace dialog reusing search state + replacement input: Enter = find next, replacement actions: replace current match + advance and replace all (with typed confirm modal reusing ConfirmKind; undo-able: replace-all routes through document edit ops so Ctrl+Z restores). **Verify:** tests + USER. **Commit:** `0562ca5` (chords: F5 replace-current fallback + primary chord; Tab/Shift+Tab field switch; confirm-modal key priority fixed over search dialog; 4 i18n keys; 8 new tests total, 138 green, 0 warnings). **USER PENDING: probar Ctrl+H.**
- [ ] **3. Ctrl+Shift+F find across all open docs** — same dialog, matches iterated in tab order; navigation may switch the active tab (per-tab cursor/scroll restore already exists); dialog shows doc title of current match. **Verify:** tests + USER. **Commit:** (pending)
- [ ] **4. Ctrl+Shift+H replace across all open docs** — replace-all across every open doc with ONE typed confirm showing counts ("Replace N in M files?"); per-doc undo still works. **Verify:** tests + USER. **Commit:** (pending)

## Keymap additions (this feature)
Ctrl+F find · Ctrl+H replace · Ctrl+Shift+F find all docs · Ctrl+Shift+H replace all docs · Enter/Shift+Enter navigate · F-key fallbacks for swallowed chords (document exact keys in README).

## Out of scope (this feature)
Case-sensitive/regex UI toggles (struct-ready only), multi-line matches, search history, project-wide search on disk, results sidebar/panel.

## Evidence log
- 2026-10-12 Feature requested by owner after milestone-1 acceptance; doc created before first write per ODD. Line-number gutter landed separately as 4cc20d1 (review approved, acknowledged).
