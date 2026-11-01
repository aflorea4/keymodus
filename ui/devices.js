// Original line drawings of supported devices, drawn for Keymodus (no vendor
// artwork, logos or wordmarks). Colours come from the theme tokens in
// styles.css, so the drawings follow light and dark mode.
//
// Each entry: { svg, ar (width / height), max (px), spots }.
// `spots` maps a control to [x%, y%, side], with side -1 = label on the left
// and 1 = label on the right. Mouse spots are keyed by CID (decimal);
// keyboard spots are keyed by "f1".."f12".

const S = 'stroke="var(--line)" stroke-width="1.4" stroke-linejoin="round"';
const SOFT = 'stroke="var(--line-soft)" stroke-width="1"';

// ---------------------------------------------------------------------------
// LIFT for Mac: right-handed vertical mouse, three-quarter view from the thumb side.
// ---------------------------------------------------------------------------

const pill = (cx, cy, w, h, rot, fill) =>
  `<rect x="${cx - w / 2}" y="${cy - h / 2}" width="${w}" height="${h}" rx="${h / 2}" transform="rotate(${rot} ${cx} ${cy})" fill="${fill}" ${S}/>`;

const wheelRidges = Array.from({ length: 6 }, (_, i) => {
  const x = 233 + i * 5;
  return `<path d="M${x} 89v10" ${SOFT} transform="rotate(22 246 94)"/>`;
}).join("");

const liftSvg = `<svg viewBox="0 0 300 345" class="device-svg" fill="none" stroke-linecap="round" aria-hidden="true">
<path d="M62 312Q160 338 258 312L256 326Q160 352 64 326Z" fill="var(--sunken)" ${S}/>
<path d="M70 318C52 262 50 196 82 140C110 90 150 52 196 34C232 20 262 34 270 70C278 110 276 200 268 262C264 292 258 312 252 318Q160 336 70 318Z" fill="var(--surface)" ${S}/>
<path d="M226 156C246 162 262 168 273 172C274 210 272 252 266 290C262 306 257 315 252 318L234 320C240 270 238 200 226 156Z" fill="var(--line-soft)" stroke="none"/>
<path d="M84 136C116 98 150 70 182 50" ${SOFT}/>
<path d="M76 226C92 204 134 194 176 204C188 240 186 282 174 318L82 318C68 288 66 254 76 226Z" fill="var(--sunken)" fill-opacity=".6" ${SOFT}/>
<path d="M98 300C120 292 150 292 168 300" ${SOFT}/>
<path d="M178 44C214 24 258 30 268 66C276 100 276 140 273 172C258 166 242 160 226 156C222 112 206 74 178 44Z" fill="var(--raised)" ${S}/>
<path d="M197 70C220 81 248 95 275 104" ${S}/>
${pill(246, 94, 36, 13, 22, "var(--sunken)")}${wheelRidges}
${pill(206, 80, 17, 9, 22, "var(--raised)")}
${pill(142, 158, 42, 13, -28, "var(--raised)")}
${pill(110, 183, 42, 13, -28, "var(--raised)")}
<rect x="150" y="327" width="22" height="3" fill="var(--volt)" stroke="none"/>
</svg>`;

// ---------------------------------------------------------------------------
// MX Mechanical Mini for Mac: compact 75% layout, top view.
// ---------------------------------------------------------------------------

const U = 50; // one key unit, horizontally
const X0 = 20; // left edge of the key field
const FROW = { y: 16, h: 36 };
const ROWS_Y = 62;
const ROW_H = 46;

// Rows below the F row as [width in units, label] pairs; each sums to 16 units.
const L = (s) => s.split(" ").map((k) => [1, k]);
const ROWS = [
  [...L("` 1 2 3 4 5 6 7 8 9 0 - ="), [2, "⌫"], [1, ""]],
  [[1.5, "⇥"], ...L("Q W E R T Y U I O P [ ]"), [1.5, "\\"], [1, ""]],
  [[1.75, "⇪"], ...L("A S D F G H J K L ; '"), [2.25, "↩"], [1, ""]],
  [[2.25, "⇧"], ...L("Z X C V B N M , . /"), [1.75, "⇧"], [1, "↑"], [1, ""]],
  [[1, "fn"], [1, "⌃"], [1.25, "⌥"], [1.25, "⌘"], [6.25, ""], [1.25, "⌘"], [1, "⌥"], [1, "←"], [1, "↓"], [1, "→"]],
];

const label = (x, y, t, size = 10, fill = "var(--text-3)") =>
  t ? `<text x="${x}" y="${y}" font-size="${size}" fill="${fill}" stroke="none" text-anchor="middle" font-family="var(--font-mono)">${t}</text>` : "";

function cap(x, y, w, h, strong) {
  return `<rect x="${x + 3}" y="${y + 3}" width="${w - 6}" height="${h - 6}" rx="3" fill="var(--raised)" ${strong ? S : SOFT}/>`;
}

function keyboardKeys() {
  const out = [];
  // F row: Esc, F1..F12, three extra keys. F keys are the remappable ones.
  for (let i = 0; i < 16; i++) {
    const x = X0 + i * U;
    const isF = i >= 1 && i <= 12;
    out.push(cap(x, FROW.y, U, FROW.h, isF));
    const t = i === 0 ? "esc" : isF ? `F${i}` : "";
    out.push(label(x + U / 2, FROW.y + 22, t, 10, isF ? "var(--text-2)" : "var(--text-3)"));
    if (i >= 1 && i <= 3) out.push(`<circle cx="${x + 11}" cy="${FROW.y + 10}" r="1.6" fill="var(--volt)" stroke="none"/>`, label(x + 39, FROW.y + 13, String(i), 7));
  }
  ROWS.forEach((row, r) => {
    let x = X0;
    const y = ROWS_Y + r * ROW_H;
    for (const [w, t] of row) {
      out.push(cap(x, y, w * U, ROW_H, false), label(x + (w * U) / 2, y + 27, t, 11));
      x += w * U;
    }
  });
  return out.join("");
}

const mxMiniSvg = `<svg viewBox="0 0 840 300" class="device-svg" fill="none" aria-hidden="true">
<rect x="2" y="2" width="836" height="296" rx="10" fill="var(--surface)" ${S}/>
<rect x="14" y="10" width="812" height="284" rx="5" fill="var(--sunken)" ${SOFT}/>
${keyboardKeys()}
</svg>`;

const pct = (v, total) => Math.round((v / total) * 1000) / 10;
const fSpots = Object.fromEntries(
  Array.from({ length: 12 }, (_, i) => [`f${i + 1}`, [pct(X0 + (i + 1) * U + U / 2, 840), pct(FROW.y + FROW.h / 2, 300), 1]]),
);

export const DEVICE_ART = {
  "046d:b031": {
    svg: liftSvg,
    ar: 300 / 345,
    max: 340,
    spots: {
      82: [pct(246, 300), pct(94, 345), 1],
      253: [pct(206, 300), pct(80, 345), -1],
      86: [pct(142, 300), pct(158, 345), -1],
      83: [pct(110, 300), pct(183, 345), -1],
    },
  },
  "046d:b36d": {
    svg: mxMiniSvg,
    ar: 840 / 300,
    max: 820,
    spots: fSpots,
    fkeyRow: pct(FROW.y + FROW.h / 2, 300),
  },
};
