# teams-rs

## Regel #1: TDD — Test-Driven Development

Immer zuerst den Test schreiben, dann den Code. Kein Feature ohne Test.

1. **Red** — Test schreiben, der fehlschlägt
2. **Green** — Minimalen Code schreiben, damit der Test passt
3. **Refactor** — Code aufräumen, Tests müssen weiter grün sein

## Learnings & Probleme

Wissensspeicher — hier kommen Erkenntnisse rein, die bei zukünftiger Arbeit helfen.

### Anti-Regression-Prozess

**Problem**: View-Logik war direkt in `view()` — ein riesiger Closure, nicht testbar. Jede Änderung konnte Features kaputtmachen ohne dass es auffiel.

**Lösung**: Darstellungslogik in testbare Pure Functions extrahieren.

- `chat_display_props(chat) -> ChatDisplayProps` berechnet alle visuellen Eigenschaften
- Die `view()` Methode liest nur noch Props und baut Widgets daraus
- **Tests prüfen die Props, nicht die Widgets**
- Jedes visuelle Feature hat einen **positiven Test** ("unread zeigt Badge") UND einen **negativen Test** ("read zeigt keine Badge")
- Bevor Code geändert wird: `cargo test` muss grün sein
- Nach jeder Änderung: `cargo test` muss grün sein

**Regel**: Nie visuelle Logik direkt in `view()` schreiben. Immer über eine testbare Funktion gehen.

### iced 0.14 Font-Fallback

**Problem**: `iced::Font { weight: Bold, ..Default::default() }` rendert als Monospace.
**Ursache**: `Default::default()` setzt `family: Family::SansSerif`. cosmic_text mappt `SansSerif` auf `"Open Sans"` (hardcoded). fontdb sucht nach `"Open Sans" Bold` — findet nichts → Fallback auf System-Monospace. Die geladene FiraSans-Bold.ttf hat family `"Fira Sans"`, nicht `"Open Sans"`, wird also nie gematcht.

### iced 0.14 Font-System — Komplettdoku

**Architektur**: iced hat KEINE Font-Handles/Objekte. Man kann nicht "lade Font X und gib mir ein Handle" machen.
Stattdessen: Descriptor-basiertes Matching über cosmic_text's fontdb.

**Ablauf**:
1. `.font(include_bytes!("font.ttf"))` in `main.rs` → registriert TTF in fontdb unter dem **eingebetteten Family-Namen** (nameID=1 aus der TTF name table)
2. Im View: `text("foo").font(Font::with_name("Fira Sans"))` → erzeugt Attrs `{ family: "Fira Sans", weight: Normal }`
3. cosmic_text sucht in fontdb: Family = "Fira Sans" AND Weight = Normal → findet Match

**Matching-Logik**: `iced_graphics/src/text.rs` → `to_attributes(font)` konvertiert `iced::Font` zu `cosmic_text::Attrs`:
- `Family::Name("Fira Sans")` → sucht Font mit family name "Fira Sans"
- `weight: Bold` → sucht Font mit weight Bold
- Alle 4 Felder (family, weight, stretch, style) müssen matchen

**Aktuelle Konfiguration** (funktioniert!):
```
fonts/FiraSans-Regular.ttf       → family="Fira Sans", subfamily="Regular"
fonts/FiraSans-Bold-Renamed.ttf  → family="Fira Sans Bold", subfamily="Regular"
```
- Regular: `Font::with_name("Fira Sans")` → O(1) Match
- Bold: `Font::with_name("Fira Sans Bold")` → O(1) Match (eigene Family!)

**WARUM die umbenannte Bold-Font**: Die originale FiraSans-Bold.ttf hat family="Fira Sans" + subfamily="Bold".
Mit `Font { family: Name("Fira Sans"), weight: Bold }` SOLLTE das matchen — tut es aber nicht zuverlässig.
Die Lösung: Bold-Font umbenannt (via fonttools) auf family="Fira Sans Bold" + subfamily="Regular".
So wird sie über `Font::with_name("Fira Sans Bold")` direkt per Family-Name gefunden, ohne Weight-Matching.

**WICHTIG — NIE MACHEN**:
- `Font::DEFAULT` oder `..Default::default()` → family wird `SansSerif` → mappt auf "Open Sans" → Fallback-Chaos
- `Family::SansSerif` irgendwo verwenden → gleiches Problem
- Immer `Font::with_name("Fira Sans")` bzw. `Font::with_name("Fira Sans Bold")` explizit angeben

### iced 0.14 Layout — height(Length::Fill) in Rows ist gefährlich

**Problem**: `container("").width(3).height(Length::Fill)` als vertikaler Indicator in einer `row![]` innerhalb eines `button()` macht die **gesamte Zeile unsichtbar** (Höhe 0).

**Ursache**: `Length::Fill` in einer Row-Zelle sagt "nimm die volle verfügbare Höhe". Aber wenn die Row in einem Button ist, hat sie keine feste Höhe → Fill wird zu 0 → gesamte Zeile kollabiert.

**Lösung**: Feste Höhe verwenden:
```rust
// FALSCH — macht Zeile unsichtbar:
container("").width(3).height(Length::Fill)

// RICHTIG — feste Höhe:
container("").width(3).height(20)
```

**Regel**: In Row-Items innerhalb von Buttons NIEMALS `height(Length::Fill)` verwenden. Immer feste Pixel-Höhe.

### iced 0.14 Scrollable mit anchor_bottom

- `anchor_bottom()` → `absolute_offset().y == 0` ist am **Boden** (neueste Messages)
- `absolute_offset_reversed().y` → `0` = ganz oben (älteste Messages sichtbar)
- `on_scroll(Message::ScrollChanged)` liefert `Viewport` mit diesen Werten

### Teams Chat Service API — Pagination

- `_metadata.backwardLink` = absolute URL für die nächste ältere Seite
- `_metadata.syncState` = URL für Polling nach neuen Messages
- Messages kommen newest-first, `pageSize=50`

### Teams Chat Service API — Unread-Erkennung

- `properties.consumptionhorizon` Format: `readMsgId;deliveredMsgId;clientMsgId`
- `lastUpdatedMessageId` = ID der neuesten Nachricht
- Unread = `lastUpdatedMessageId > consumptionhorizon[0]`
- Kein direkter Unread-Count in der API — muss über Messages gezählt werden (id > horizon)
- Message-IDs sind Timestamps (nicht Sequenz-Nummern)

### iced 0.14 Button-Styling

- `button.style(|theme, status| { ... })` für Hover/Selected/Pressed States
- `status` ist `iced::widget::button::Status::Hovered | Pressed | Disabled | Active`
- Transparenter Hintergrund + Hover-Highlight = Teams-Look

### iced 0.14 Widget-API

- `Space::new().width(Length::Fill)` als Spacer in Rows (kein `horizontal_space()`)
- `container("")` für leere Platzhalter (z.B. vertikaler Strich-Indikator)
- `tooltip(widget, content, Position::Top)` für Tooltip-Overlays
- `Column::with_children(vec)` / `Row::with_children(vec)` für dynamische Listen

### REGEL: Browser-Nutzung — 7-Schritte-Prozess

**Problem**: Claude hat den echten Chrome des Users geöffnet (`/Applications/Google Chrome.app`) statt die Test-Instanz (`headless_chrome`). Das riskiert User-Sessions, Tabs, Profildaten.

**7-Schritte-Prozess — IMMER befolgen:**

1. **NUR `headless_chrome` verwenden** — Niemals `/Applications/Google Chrome.app` direkt aufrufen. Kein `Command::new("Google Chrome")`, kein Shell-Script mit Chrome-Binary-Pfad.

2. **Kein `--user-data-dir` auf das echte Profil** — Nie `~/Library/Application Support/Google/Chrome` referenzieren. Wenn ein Profil nötig ist, immer `/tmp/` verwenden.

3. **Kein `pkill Chrome`** — Niemals Chrome-Prozesse killen. Der User könnte eigene Chrome-Fenster offen haben.

4. **Vor jedem Browser-Start prüfen**: Wird `headless_chrome::Browser::new()` verwendet? Wenn nein → STOPP.

5. **Kein Proxy über den echten Chrome** — Proxy-Capture nur über `headless_chrome` oder standalone Tools (mitmdump allein), nie über den System-Chrome.

6. **SSO funktioniert auch in headless_chrome** — Chrome meldet sich automatisch an. Kein Grund, den echten Chrome zu nutzen. Das wurde bereits bestätigt.

7. **Bei Fehlern NICHT eskalieren** — Wenn `headless_chrome` nicht funktioniert, einen anderen Ansatz wählen (z.B. Web-Recherche, API-Dokumentation lesen). NIEMALS als "Lösung" den echten Chrome öffnen.

**Zusammenfassung**: `headless_chrome` = OK. Alles andere = VERBOTEN.

### Minifiziertes JavaScript analysieren

**Problem**: Heruntergeladene JS-Bundles von Web-Apps (z.B. Teams) sind minifiziert und unlesbar.

**Lösung**: Immer zuerst mit `prettier` oder `js-beautify` formatieren, dann analysieren.

```bash
npx prettier --write datei.js
# oder
python3 -m jsbeautifier datei.js > datei_formatted.js
```

**Regel**: Nie minifizierten Code direkt durchsuchen. Erst formatieren, dann lesen.
