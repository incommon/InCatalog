# Third-party marks

These files are other organizations' marks. They are not licensed under this
project's Apache-2.0 license (§6 excludes trademarks). Don't redraw, recolor or
re-space them.

- **SeamlessAccess's logomark** is used as SeamlessAccess publishes it for reuse. A
  fork should check SeamlessAccess's terms before using it.
- **The InCommon and Internet2 logotypes** in the InCommon lockup are Internet2's
  trademarks (InCommon is an LLC operated by Internet2), used with Internet2's
  permission. A fork must remove them.

| File | What | Source | Retrieved | SHA-256 |
|---|---|---|---|---|
| `incommon-poweredbyi2-lockup-orange.png` | The InCommon logotype with the "Powered by Internet2" sublogotype beneath it, orange InCommon, black and red Internet2, for light backgrounds. 2883×1113 PNG, cropped to the artwork | Internet2 deck assets kept in Nicole Roy's ClaudeOfStaff vault (`_attachments/`, file dated 2026-06-24), where the original source isn't recorded. No version number. InCommon publishes the same lockup at 164×63 on its [Trademark and Logos](https://incommon.org/incommon-trademark-and-logos/) page | 2026-11-01 | `6732eccf349e629bbcbe30d870376d37e89cd01e812a2cb759d8c072ae9a72d1` |
| `incommon-poweredbyi2-lockup-white.png` | The same lockup, all white, for dark backgrounds | As above (file dated 2026-06-24) | 2026-11-01 | `48379b071c79706c204116f043761ae49bd33365a9d8b9696108664a90e0c04a` |
| `sa-white.svg` | SeamlessAccess's open-door institution logomark, white, for the "Access through your institution" button | `https://service.seamlessaccess.org/sa-white.svg`, listed under "Reusable Assets" in SeamlessAccess's [States, Styles, and Assets](https://seamlessaccess.atlassian.net/wiki/spaces/DOCUMENTAT/pages/458783/States+Styles+and+Assets) (page updated 2023-06-30; file last modified 2026-10-14) | 2026-11-01 | `130a85a1b60f243e12795d5f0b6b0763102343080f78f1dedc65ef61044e6bb0` |

The button around the logomark follows the same page's static button specification:
the logomark 28px high, centered in the first 50px, a 1px white divider 32px high,
"Access through your institution" in Arial bold 14px, a 5px radius, 256px to 328px
wide, and the 1px and 4px focus and hover ring.

The InCommon lockup floats, centered, at the bottom of every page, above the footer
(`.floater` in `web/index.html` and `web/styles.css`). The orange version shows on the
light theme's cream and the white one on the dark theme's navy. They swap with the
theme tokens' selectors, so `data-theme` overrides the system setting. Neither
InCommon's page nor Internet2's media resources give clear-space or minimum-size
rules, so the site sets its own: the lockup is 64px high, at least as large as
incommon.org shows it (63px), so "Powered by" stays legible, with a quarter of its
height (16px) clear on every side. Once the page scrolls, the lockup shrinks to 32px
inside the footer's navy bar, white in either theme, so the floater is no taller
than the bar; it grows back at the top of the page. Replace both PNGs
with SVGs and these rules with the guidelines' own once Internet2's marketing team
supplies them.
