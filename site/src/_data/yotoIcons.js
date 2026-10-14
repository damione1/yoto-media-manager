/**
 * 16 × 16 pixel icons in the style of Yoto's icon catalogue, for the app
 * mocks: what a track, a media or the icon picker shows once an icon is
 * chosen. The app draws real Yoto icons (16 × 16 images) on a pastel tile;
 * these are drawn here so the site ships no Yoto artwork.
 *
 * Each icon is 16 rows of 16 characters; "." is transparent, any other
 * character is a colour from PALETTE. Exported as runs of same-colour
 * pixels per row, which the yicon() macro turns into <rect>s.
 */
const PALETTE = {
  Y: "#FFC93C", O: "#FF9F43", R: "#F2545B", P: "#FF8FB1", G: "#2FB463",
  g: "#8FDB6E", B: "#4C8DF0", b: "#9CC7FF", W: "#FFFFFF", w: "#C9C1B6", K: "#2E2A28",
  N: "#9A6334", L: "#B28DF7", V: "#7A52D9", S: "#5B6770",
};

const ART = {
  moon: [
    "................",
    ".....YYYY.......",
    "...YYYYY........",
    "..YYYYY.........",
    "..YYYY......W...",
    ".YYYY......WWW..",
    ".YYYY.......W...",
    ".YYYY...........",
    ".YYYYY..........",
    ".YYYYY..........",
    "..YYYYY.........",
    "..YYYYYYY.....YY",
    "...YYYYYYYYYYYY.",
    "....YYYYYYYYYY..",
    "......YYYYYY....",
    "................",
  ],
  star: [
    "................",
    ".......YY.......",
    ".......YY.......",
    "......YYYY......",
    "......YYYY......",
    ".YYYYYYYYYYYYYY.",
    "..YYYYYYYYYYYY..",
    "...YYYYYYYYYY...",
    "....YYYYYYYY....",
    "....YYYYYYYY....",
    "...YYYYOOYYYY...",
    "...YYYO..OYYY...",
    "..YYYO....OYYY..",
    "..YYO......OYY..",
    "..YO........OY..",
    "................",
  ],
  heart: [
    "................",
    "................",
    "..RRRR....RRRR..",
    ".RRWWRR..RRRRRR.",
    ".RWWRRRRRRRRRRR.",
    ".RWRRRRRRRRRRRR.",
    ".RRRRRRRRRRRRRR.",
    ".RRRRRRRRRRRRRR.",
    "..RRRRRRRRRRRR..",
    "...RRRRRRRRRR...",
    "....RRRRRRRR....",
    ".....RRRRRR.....",
    "......RRRR......",
    ".......RR.......",
    "................",
    "................",
  ],
  sun: [
    ".......Y........",
    "..Y....Y....Y...",
    "...Y.......Y....",
    "......OOOO......",
    ".....OYYYYO.....",
    "....OYYYYYYO....",
    "YY..OYYYYYYO..YY",
    "....OYYYYYYO....",
    "....OYYYYYYO....",
    ".....OYYYYO.....",
    "......OOOO......",
    "...Y.......Y....",
    "..Y....Y....Y...",
    ".......Y........",
    "................",
    "................",
  ],
  sheep: [
    "................",
    "....wwwwwww.....",
    "..wwWWWWWWWww...",
    ".wWWWWWWWWWWWw..",
    "wWWWWWWWWWWWKKK.",
    "wWWWWWWWWWWKKWKK",
    "wWWWWWWWWWWKKKKK",
    "wWWWWWWWWWWWKKK.",
    ".wWWWWWWWWWWWw..",
    "..wwwwwwwwwww...",
    "...K.K...K.K....",
    "...K.K...K.K....",
    "...K.K...K.K....",
    "..KK.KK.KK.KK...",
    "................",
    "................",
  ],
  boat: [
    "................",
    ".......K........",
    ".......KRR......",
    ".......KRRRR....",
    ".......KRRRRRR..",
    "......WKRRRRR...",
    ".....WWKRRR.....",
    "....WWWKR.......",
    "...WWWWK........",
    "..WWWWWK........",
    "KKKKKKKKKKKKKKKK",
    ".NNNNNNNNNNNNNN.",
    "..NNNNNNNNNNNN..",
    "BBBNNNNNNNNNNBBB",
    "bBBBBBBBBBBBBBBb",
    "..bbbb....bbbb..",
  ],
  mouse: [
    "................",
    "................",
    "..GG............",
    ".GPPG...........",
    ".GPPGGGGG.......",
    "..GGGGGGGGG.....",
    ".GGKGGGGGGGG....",
    "PGGGGGGGGGGGG...",
    ".GGGGGGGGGGGGG..",
    "..GGGGGGGGGGGGG.",
    "...GGGGGGGGGGGGS",
    "....G..G.....G.S",
    "...GG.GG....GG.S",
    "..............S.",
    "............SS..",
    "................",
  ],
  bell: [
    ".......NN.......",
    "......OYYO......",
    ".....OYYYYO.....",
    ".....YYWYYY.....",
    "....YYWYYYYY....",
    "....YYWYYYYY....",
    "....YYYYYYYY....",
    "...YYYYYYYYYY...",
    "...YYYYYYYYYY...",
    "..YYYYYYYYYYYY..",
    ".OOOOOOOOOOOOOO.",
    ".OOOOOOOOOOOOOO.",
    "......NNNN......",
    ".......NN.......",
    "................",
    "................",
  ],
  note: [
    "................",
    ".....LLLLLLLLL..",
    ".....LLLLLLLLL..",
    ".....L.......L..",
    ".....L.......L..",
    ".....L.......L..",
    ".....L.......L..",
    ".....L.......L..",
    ".....L....VVVL..",
    ".....L...VVVVL..",
    "..VVVL...VVVV...",
    ".VVVVL....VV....",
    ".VVVV...........",
    "..VV............",
    "................",
    "................",
  ],
  tree: [
    ".......G........",
    "......GGG.......",
    ".....GgGGG......",
    "....GGGGGGG.....",
    "......GGG.......",
    ".....GGGgGG.....",
    "....GGGGGGGG....",
    "...GgGGGGGGGG...",
    ".....GGGGG......",
    "....GGGGGgGG....",
    "...GGGGGGGGGG...",
    "..GGGgGGGGGGGG..",
    ".GGGGGGGGGGGGGG.",
    ".......NN.......",
    ".......NN.......",
    "......NNNN......",
  ],
  cloud: [
    "................",
    "................",
    "......wwww......",
    ".....wWWWWw.....",
    "...wwWWWWWWww...",
    "..wWWWWWWWWWWw..",
    ".wWWWWWWWWWWWWw.",
    "wWWWWWWWWWWWWWWw",
    "wWWWWWWWWWWWWWWw",
    ".wwwwwwwwwwwwww.",
    "................",
    "...B...B...B....",
    "..B...B...B.....",
    ".B...B...B......",
    "................",
    "................",
  ],
  rainbow: [
    "................",
    "................",
    "....RRRRRRRR....",
    "..RRRRRRRRRRRR..",
    ".RROOOOOOOOOORR.",
    ".ROOYYYYYYYYOOR.",
    "RROYYGGGGGGYYORR",
    "ROYYGGBBBBGGYYOR",
    "ROYGGBB..BBGGYOR",
    "ROYGBB....BBGYOR",
    "ROYGB......BGYOR",
    "ROYGB......BGYOR",
    "WWWWW......WWWWW",
    ".WWW........WWW.",
    "................",
    "................",
  ],
};

function runs(rows) {
  const out = [];
  rows.forEach((row, y) => {
    let x = 0;
    while (x < row.length) {
      const ch = row[x];
      let w = 1;
      while (x + w < row.length && row[x + w] === ch) w += 1;
      if (ch !== ".") out.push({ x, y, w, c: PALETTE[ch] });
      x += w;
    }
  });
  return out;
}

const icons = {};
for (const [name, rows] of Object.entries(ART)) {
  if (rows.length !== 16 || rows.some((r) => r.length !== 16 || /[^.]/.test(r) && [...r].some((c) => c !== "." && !PALETTE[c]))) {
    throw new Error(`yotoIcons: ${name} is not 16 × 16 or uses an unknown colour`);
  }
  icons[name] = runs(rows);
}

export default icons;
