# Browser cases for the folio theme

Cases for a person or a headless browser. Each result goes in the pull request that runs it, with the browser and version.

Setup: serve `examples/directives` and a page with 100 or more headings. Phone width is 390 x 844; desktop is 1280 x 800.

## Keyboard

| Id | Steps | Expected |
|---|---|---|
| K1 | Load a page, press Tab once. Press Enter. | Focus is on the skip link with a 2 px outline. After Enter the page moves to the main content. |
| K2 | Press Tab repeatedly from the skip link. | Order is menu button, brand link, theme toggle, content. A closed drawer receives no focus. |
| K3 | On the menu button press Enter, then Escape. | `aria-expanded` goes `true` then `false`, and the menu button holds focus. |
| K4 | Open the drawer, focus a link inside it, press Escape. | The drawer closes and focus returns to the menu button. |
| K5 | Tab to the active tab button, press ArrowRight, ArrowLeft, End, Home. | Selection and focus move together and wrap. A second group with the same `sync` follows, and `localStorage` holds the title. |
| K6 | From the active tab button press Tab. | Focus enters the tab panel. |
| K7 | Disable JavaScript and open the tabs page. | Every panel shows under its title and no tab list exists. |
| K8 | Tab to a `details` summary, press Enter and Space. | The block opens and closes, the summary shows an outline, nothing overflows. |
| K9 | Tab to a card title. | The whole card shows the outline. Enter follows the link. |
| K10 | Tab through the code page. | Every code block takes focus, and ArrowRight scrolls a long one. |
| K11 | Focus the theme toggle, press Enter, reload. | The scheme changes, is kept after reload, and does not flash. |
| K12 | Tab through a long page. | The focused element is never under the top bar. |

## Reduced motion

| Id | Steps | Expected |
|---|---|---|
| M1 | Emulate `reduce`, open the drawer. | `transition-duration` of the drawer is about 0.01 ms and it is in place at once. |
| M2 | Emulate `no-preference`, open and close the drawer. | Opening takes 200 ms and closing 180 ms. |
| M3 | Emulate `reduce`, close the drawer. | The drawer is `visibility: hidden` within 20 ms. |
| M4 | Emulate `no-preference`, read the style sheets. | An `@view-transition` rule is present and a page change animates. |
| M5 | Set `data-motion="off"` on the root with `no-preference`. | No transition, no animation, no view transition, and jumps to an anchor are instant. |
| M6 | Emulate `reduce`, focus a card. | The border change takes about 0.01 ms. |

## Dark scheme at 390 px

Run on cards, columns, tabs, steps and details, once with `prefers-color-scheme: dark` and once with `data-theme="dark"` under a light system.

| Id | Steps | Expected |
|---|---|---|
| D1 | Read `scrollWidth` of the root. | It equals 390. |
| D2 | Measure contrast of body text, card title, inactive tab, summary, step number and column text. | At least 4.5 : 1. Borders of cards, tab list and details are at least 3 : 1. |
| D3 | Focus a tab button, a summary, a card and the menu button. | The outline is at least 3 : 1 against the background. |
| D4 | Look at the active tab. | It is told apart by more than colour. |
| D5 | Look at the steps. | The number circle and the vertical line are visible. |
| D6 | Press the theme toggle under a light system. | The icons swap and no light colour stays in a directive. |

## Console

| Id | Steps | Expected |
|---|---|---|
| C1 | Open the developer console of a Chromium or Firefox browser and load any page. | Two messages appear once: the logo, a shelf with its cat and the word `anaquel`, which builds itself in about 4 s with the letters drawn one by one and then keeps the tail moving, and the line `Hello from Team! This is Anaquel Dev Team.`. No error or warning. |
| C2 | Emulate `reduce`, or set `data-motion="off"`, and load a page. | The same image appears and it is still. |

## Anchor scroll

Setup: `no-preference` and no `data-motion` unless the case says otherwise.

| Id | Steps | Expected |
|---|---|---|
| A1 | Click a table of contents link to a heading 5 000 px or more down. | The page scrolls in 200 to 400 ms with a slowing end, and stops at the heading minus the top bar, or at the end of the page. |
| A2 | After A1 read `location.hash`, then go back. | The hash is the heading, one history entry was added, and Back restores the earlier hash and scroll position within 2 px. |
| A3 | After A1 read `document.activeElement`, then Tab away. | The heading had focus and `tabindex="-1"`, and the attribute is gone after it loses focus. |
| A4 | Emulate `reduce`, click the same link. | The click is not prevented, `scroll-behavior` is `auto`, and the first frame is at the target. |
| A5 | Set `data-motion="off"`, click the same link. | Same result as A4. |
| A6 | Click with Ctrl, Meta, Shift, Alt and the middle button. | None is prevented. |
| A7 | Click a link to `#missing`. | The click is not prevented and no history entry is added. |
| A8 | Click a link to another page and a link with `target="_blank"`. | Neither is prevented. |
| A9 | Start A1, then use the wheel, touch, a mouse press, or ArrowDown. | The animation stops where it is. |
| A10 | Click the same link twice. | The second click adds no history entry. |
| A11 | Point a link at a heading inside a hidden tab panel. | The click is not prevented. |
| A12 | Disable JavaScript and click the link. | The browser jumps to the heading, smoothly under `no-preference` by the style sheet. |
| A13 | After A1 go Back and Forward, once slowly and once quickly (a second step before the first settles). | Each step is instant and lands within 2 px of the earlier position. The root keeps an inline `scroll-behavior` of `auto`. |
