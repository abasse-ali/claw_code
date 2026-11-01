// Couche « design » de Claw : les retouches faites à la main dans l'éditeur visuel de Claw Web
// (couleurs, tailles, espacements, textes, icônes, ordre, widgets ajoutés, thème), rangées dans
// claw-design.json à côté de index.html, et appliquées par-dessus l'appli à chaque affichage.
// Demandé le 1er novembre 2026 : « une interface graphique (glisser-déposer, modifier taille,
// couleur des widgets) ». Le code de l'appli reste celui de Claw ; ce fichier passe après lui,
// même quand l'appli redessine ses écrans.
//
// Dans l'éditeur (Claw Web l'ouvre dans un cadre), `clawDesign.edit(hôte)` ajoute la sélection
// au toucher, le cadre de l'élément choisi, la poignée de taille et le glisser pour déplacer.
// Icônes : Lucide (licence ISC, https://lucide.dev).
(() => {
  "use strict";
  if (window.clawDesign) return;
  const VERSION = 1;
  const OWN = "data-claw-own";
  const ICONS = {"house":"<path d=\"M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8\"/><path d=\"M3 10a2 2 0 0 1 .709-1.528l7-6a2 2 0 0 1 2.582 0l7 6A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z\"/>","search":"<path d=\"m21 21-4.34-4.34\"/><circle cx=\"11\" cy=\"11\" r=\"8\"/>","plus":"<path d=\"M5 12h14\"/><path d=\"M12 5v14\"/>","minus":"<path d=\"M5 12h14\"/>","x":"<path d=\"M18 6 6 18\"/><path d=\"m6 6 12 12\"/>","check":"<path d=\"M20 6 9 17l-5-5\"/>","settings":"<path d=\"M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915\"/><circle cx=\"12\" cy=\"12\" r=\"3\"/>","user":"<path d=\"M19 21v-2a4 4 0 0 0-4-4H9a4 4 0 0 0-4 4v2\"/><circle cx=\"12\" cy=\"7\" r=\"4\"/>","users":"<path d=\"M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2\"/><path d=\"M16 3.128a4 4 0 0 1 0 7.744\"/><path d=\"M22 21v-2a4 4 0 0 0-3-3.87\"/><circle cx=\"9\" cy=\"7\" r=\"4\"/>","heart":"<path d=\"M2 9.5a5.5 5.5 0 0 1 9.591-3.676.56.56 0 0 0 .818 0A5.49 5.49 0 0 1 22 9.5c0 2.29-1.5 4-3 5.5l-5.492 5.313a2 2 0 0 1-3 .019L5 15c-1.5-1.5-3-3.2-3-5.5\"/>","star":"<path d=\"M11.525 2.295a.53.53 0 0 1 .95 0l2.31 4.679a2.123 2.123 0 0 0 1.595 1.16l5.166.756a.53.53 0 0 1 .294.904l-3.736 3.638a2.123 2.123 0 0 0-.611 1.878l.882 5.14a.53.53 0 0 1-.771.56l-4.618-2.428a2.122 2.122 0 0 0-1.973 0L6.396 21.01a.53.53 0 0 1-.77-.56l.881-5.139a2.122 2.122 0 0 0-.611-1.879L2.16 9.795a.53.53 0 0 1 .294-.906l5.165-.755a2.122 2.122 0 0 0 1.597-1.16z\"/>","bell":"<path d=\"M10.268 21a2 2 0 0 0 3.464 0\"/><path d=\"M3.262 15.326A1 1 0 0 0 4 17h16a1 1 0 0 0 .74-1.673C19.41 13.956 18 12.499 18 8A6 6 0 0 0 6 8c0 4.499-1.411 5.956-2.738 7.326\"/>","calendar":"<path d=\"M8 2v3\"/><path d=\"M16 2v3\"/><rect x=\"3\" y=\"3\" width=\"18\" height=\"18\" rx=\"2\"/><path d=\"M3 9h18\"/>","clock":"<circle cx=\"12\" cy=\"12\" r=\"10\"/><path d=\"M12 6v6l4 2\"/>","map-pin":"<path d=\"M20 10c0 4.993-5.539 10.193-7.399 11.799a1 1 0 0 1-1.202 0C9.539 20.193 4 14.993 4 10a8 8 0 0 1 16 0\"/><circle cx=\"12\" cy=\"10\" r=\"3\"/>","map":"<path d=\"M14.106 5.553a2 2 0 0 0 1.788 0l3.659-1.83A1 1 0 0 1 21 4.619v12.764a1 1 0 0 1-.553.894l-4.553 2.277a2 2 0 0 1-1.788 0l-4.212-2.106a2 2 0 0 0-1.788 0l-3.659 1.83A1 1 0 0 1 3 19.381V6.618a1 1 0 0 1 .553-.894l4.553-2.277a2 2 0 0 1 1.788 0z\"/><path d=\"M15 5.764v15\"/><path d=\"M9 3.236v15\"/>","phone":"<path d=\"M13.832 16.568a1 1 0 0 0 1.213-.303l.355-.465A2 2 0 0 1 17 15h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2A18 18 0 0 1 2 4a2 2 0 0 1 2-2h3a2 2 0 0 1 2 2v3a2 2 0 0 1-.8 1.6l-.468.351a1 1 0 0 0-.292 1.233 14 14 0 0 0 6.392 6.384\"/>","mail":"<path d=\"m22 7-8.991 5.727a2 2 0 0 1-2.009 0L2 7\"/><rect x=\"2\" y=\"4\" width=\"20\" height=\"16\" rx=\"2\"/>","message-circle":"<path d=\"M2.992 16.342a2 2 0 0 1 .094 1.167l-1.065 3.29a1 1 0 0 0 1.236 1.168l3.413-.998a2 2 0 0 1 1.099.092 10 10 0 1 0-4.777-4.719\"/>","send":"<path d=\"M14.536 21.686a.5.5 0 0 0 .937-.024l6.5-19a.496.496 0 0 0-.635-.635l-19 6.5a.5.5 0 0 0-.024.937l7.93 3.18a2 2 0 0 1 1.112 1.11z\"/><path d=\"m21.854 2.147-10.94 10.939\"/>","share-2":"<circle cx=\"18\" cy=\"5\" r=\"3\"/><circle cx=\"6\" cy=\"12\" r=\"3\"/><circle cx=\"18\" cy=\"19\" r=\"3\"/><line x1=\"8.59\" x2=\"15.42\" y1=\"13.51\" y2=\"17.49\"/><line x1=\"15.41\" x2=\"8.59\" y1=\"6.51\" y2=\"10.49\"/>","download":"<path d=\"M12 15V3\"/><path d=\"M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4\"/><path d=\"m7 10 5 5 5-5\"/>","upload":"<path d=\"M12 3v12\"/><path d=\"m17 8-5-5-5 5\"/><path d=\"M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4\"/>","trash":"<path d=\"M10 11v6\"/><path d=\"M14 11v6\"/><path d=\"M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6\"/><path d=\"M3 6h18\"/><path d=\"M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2\"/>","pencil":"<path d=\"M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z\"/><path d=\"m15 5 4 4\"/>","copy":"<rect width=\"14\" height=\"14\" x=\"8\" y=\"8\" rx=\"2\" ry=\"2\"/><path d=\"M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2\"/>","camera":"<path d=\"M13.997 4a2 2 0 0 1 1.76 1.05l.486.9A2 2 0 0 0 18.003 7H20a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V9a2 2 0 0 1 2-2h1.997a2 2 0 0 0 1.759-1.048l.489-.904A2 2 0 0 1 10.004 4z\"/><circle cx=\"12\" cy=\"13\" r=\"3\"/>","image":"<rect width=\"18\" height=\"18\" x=\"3\" y=\"3\" rx=\"2\" ry=\"2\"/><circle cx=\"9\" cy=\"9\" r=\"2\"/><path d=\"m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21\"/>","shopping-cart":"<path d=\"m2.05 2.05 1.099-.028a1 1 0 0 1 1.008.815l2.69 14.347A1 1 0 0 0 7.83 18H18\"/><path d=\"M4.563 5h16.435a1 1 0 0 1 .981 1.204l-1.026 6.226A2 2 0 0 1 18.962 14H6.25\"/><circle cx=\"18\" cy=\"20\" r=\"2\"/><circle cx=\"8\" cy=\"20\" r=\"2\"/>","shopping-bag":"<path d=\"M16 10a4 4 0 0 1-8 0\"/><path d=\"M3.103 6.034h17.794\"/><path d=\"M3.4 5.467a2 2 0 0 0-.4 1.2V20a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V6.667a2 2 0 0 0-.4-1.2l-2-2.667A2 2 0 0 0 17 2H7a2 2 0 0 0-1.6.8z\"/>","store":"<path d=\"M15 21v-5a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v5\"/><path d=\"M17.774 10.31a1.12 1.12 0 0 0-1.549 0 2.5 2.5 0 0 1-3.451 0 1.12 1.12 0 0 0-1.548 0 2.5 2.5 0 0 1-3.452 0 1.12 1.12 0 0 0-1.549 0 2.5 2.5 0 0 1-3.77-3.248l2.889-4.184A2 2 0 0 1 7 2h10a2 2 0 0 1 1.653.873l2.895 4.192a2.5 2.5 0 0 1-3.774 3.244\"/><path d=\"M4 10.95V19a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-8.05\"/>","wallet":"<path d=\"M19 7V4a1 1 0 0 0-1-1H5a2 2 0 0 0 0 4h15a1 1 0 0 1 1 1v4h-3a2 2 0 0 0 0 4h3a1 1 0 0 0 1-1v-2a1 1 0 0 0-1-1\"/><path d=\"M3 5v14a2 2 0 0 0 2 2h15a1 1 0 0 0 1-1v-4\"/>","credit-card":"<rect width=\"20\" height=\"14\" x=\"2\" y=\"5\" rx=\"2\"/><line x1=\"2\" x2=\"22\" y1=\"10\" y2=\"10\"/><path d=\"M6 14h2\"/>","banknote":"<rect width=\"20\" height=\"12\" x=\"2\" y=\"6\" rx=\"2\"/><circle cx=\"12\" cy=\"12\" r=\"2\"/><path d=\"M6 12h.01M18 12h.01\"/>","piggy-bank":"<path d=\"M11 17h3v2a1 1 0 0 0 1 1h2a1 1 0 0 0 1-1v-3a3.16 3.16 0 0 0 2-2h1a1 1 0 0 0 1-1v-2a1 1 0 0 0-1-1h-1a5 5 0 0 0-2-4V3a4 4 0 0 0-3.2 1.6l-.3.4H11a6 6 0 0 0-6 6v1a5 5 0 0 0 2 4v3a1 1 0 0 0 1 1h2a1 1 0 0 0 1-1z\"/><path d=\"M16 10h.01\"/><path d=\"M2 8v1a2 2 0 0 0 2 2h1\"/>","chart-column":"<path d=\"M3 3v16a2 2 0 0 0 2 2h16\"/><path d=\"M18 17V9\"/><path d=\"M13 17V5\"/><path d=\"M8 17v-3\"/>","chart-pie":"<path d=\"M21 12c.552 0 1.005-.449.95-.998a10 10 0 0 0-8.953-8.951c-.55-.055-.998.398-.998.95v8a1 1 0 0 0 1 1z\"/><path d=\"M21.21 15.89A10 10 0 1 1 8 2.83\"/>","trending-up":"<path d=\"M16 7h6v6\"/><path d=\"m22 7-8.5 8.5-5-5L2 17\"/>","trending-down":"<path d=\"M16 17h6v-6\"/><path d=\"m22 17-8.5-8.5-5 5L2 7\"/>","list":"<path d=\"M3 5h.01\"/><path d=\"M3 12h.01\"/><path d=\"M3 19h.01\"/><path d=\"M8 5h13\"/><path d=\"M8 12h13\"/><path d=\"M8 19h13\"/>","list-checks":"<path d=\"M13 5h8\"/><path d=\"M13 12h8\"/><path d=\"M13 19h8\"/><path d=\"m3 17 2 2 4-4\"/><path d=\"m3 7 2 2 4-4\"/>","square-check-big":"<path d=\"M21 10.656V19a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h12.344\"/><path d=\"m9 11 3 3L22 4\"/>","circle":"<circle cx=\"12\" cy=\"12\" r=\"10\"/>","info":"<circle cx=\"12\" cy=\"12\" r=\"10\"/><path d=\"M12 16v-4\"/><path d=\"M12 8h.01\"/>","circle-alert":"<circle cx=\"12\" cy=\"12\" r=\"10\"/><line x1=\"12\" x2=\"12\" y1=\"8\" y2=\"12\"/><line x1=\"12\" x2=\"12.01\" y1=\"16\" y2=\"16\"/>","help":"<circle cx=\"12\" cy=\"12\" r=\"10\"/><path d=\"M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3\"/><path d=\"M12 17h.01\"/>","lock":"<rect width=\"18\" height=\"11\" x=\"3\" y=\"11\" rx=\"2\" ry=\"2\"/><path d=\"M7 11V7a5 5 0 0 1 10 0v4\"/>","lock-open":"<rect width=\"18\" height=\"11\" x=\"3\" y=\"11\" rx=\"2\" ry=\"2\"/><path d=\"M7 11V7a5 5 0 0 1 9.9-1\"/>","eye":"<path d=\"M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0\"/><circle cx=\"12\" cy=\"12\" r=\"3\"/>","eye-off":"<path d=\"M10.733 5.076a10.744 10.744 0 0 1 11.205 6.575 1 1 0 0 1 0 .696 10.747 10.747 0 0 1-1.444 2.49\"/><path d=\"M14.084 14.158a3 3 0 0 1-4.242-4.242\"/><path d=\"M17.479 17.499a10.75 10.75 0 0 1-15.417-5.151 1 1 0 0 1 0-.696 10.75 10.75 0 0 1 4.446-5.143\"/><path d=\"m2 2 20 20\"/>","sun":"<circle cx=\"12\" cy=\"12\" r=\"4\"/><path d=\"M12 2v2\"/><path d=\"M12 20v2\"/><path d=\"m4.93 4.93 1.41 1.41\"/><path d=\"m17.66 17.66 1.41 1.41\"/><path d=\"M2 12h2\"/><path d=\"M20 12h2\"/><path d=\"m6.34 17.66-1.41 1.41\"/><path d=\"m19.07 4.93-1.41 1.41\"/>","moon":"<path d=\"M20.985 12.486a9 9 0 1 1-9.473-9.472c.405-.022.617.46.402.803a6 6 0 0 0 8.268 8.268c.344-.215.825-.004.803.401\"/>","cloud":"<path d=\"M17.5 19H9a7 7 0 1 1 6.71-9h1.79a4.5 4.5 0 1 1 0 9Z\"/>","zap":"<path d=\"M15.914 4a1.5 1.5 0 00-2.474-1.561l-9 9A1.5 1.5 0 005.5 14h4.002a.5.5 0 01.471.666L8.086 20a1.5 1.5 0 002.475 1.56l9-9A1.5 1.5 0 0018.5 10h-3.997a.5.5 0 01-.472-.667z\"/>","flame":"<path d=\"M12 3q1 4 4 6.5t3 5.5a1 1 0 0 1-14 0 5 5 0 0 1 1-3 1 1 0 0 0 5 0c0-2-1.5-3-1.5-5q0-2 2.5-4\"/>","leaf":"<path d=\"M11 20a10 10 0 0010-10 25.9 25.9 0 00-1.04-7.281 1 1 0 00-1.755-.325C15.833 5.5 13 5.5 9.8 6.1A7 7 0 0011 20\"/><path d=\"M2 21a5 5 0 012.911-4.544C7.613 15.212 8.351 15.24 11 13\"/>","coffee":"<path d=\"M10 2v2\"/><path d=\"M14 2v2\"/><path d=\"M16 8a1 1 0 0 1 1 1v8a4 4 0 0 1-4 4H7a4 4 0 0 1-4-4V9a1 1 0 0 1 1-1h14a4 4 0 1 1 0 8h-1\"/><path d=\"M6 2v2\"/>","utensils":"<path d=\"M3 2v7c0 1.1.9 2 2 2h4a2 2 0 0 0 2-2V2\"/><path d=\"M7 2v20\"/><path d=\"M21 15V2a5 5 0 0 0-5 5v6c0 1.1.9 2 2 2h3Zm0 0v7\"/>","dumbbell":"<path d=\"M17.596 12.768a2 2 0 1 0 2.829-2.829l-1.768-1.767a2 2 0 0 0 2.828-2.829l-2.828-2.828a2 2 0 0 0-2.829 2.828l-1.767-1.768a2 2 0 1 0-2.829 2.829z\"/><path d=\"m2.5 21.5 1.4-1.4\"/><path d=\"m20.1 3.9 1.4-1.4\"/><path d=\"M5.343 21.485a2 2 0 1 0 2.829-2.828l1.767 1.768a2 2 0 1 0 2.829-2.829l-6.364-6.364a2 2 0 1 0-2.829 2.829l1.768 1.767a2 2 0 0 0-2.828 2.829z\"/><path d=\"m9.6 14.4 4.8-4.8\"/>","activity":"<path d=\"M22 12h-2.48a2 2 0 0 0-1.93 1.46l-2.35 8.36a.25.25 0 0 1-.48 0L9.24 2.18a.25.25 0 0 0-.48 0l-2.35 8.36A2 2 0 0 1 4.49 12H2\"/>","music":"<path d=\"M21 16V3a1 1 0 0 0-1.28-.96l-10 2.917a1 1 0 0 0-.72.96V19\"/><circle cx=\"18\" cy=\"16\" r=\"3\"/><circle cx=\"6\" cy=\"19\" r=\"3\"/>","play":"<path d=\"M5 5a2 2 0 0 1 3.008-1.728l11.997 6.998a2 2 0 0 1 .003 3.458l-12 7A2 2 0 0 1 5 19z\"/>","pause":"<rect x=\"14\" y=\"3\" width=\"5\" height=\"18\" rx=\"1\"/><rect x=\"5\" y=\"3\" width=\"5\" height=\"18\" rx=\"1\"/>","book-open":"<path d=\"M12 5v16\"/><path d=\"M20.001 19A2 2 0 0022 17V5a2 2 0 00-1.999-2L16 3.002A5 5 0 0012 5a5 5 0 00-4-2H4a2 2 0 00-2 2v12a2 2 0 001.999 2H8a5 5 0 014 2 5 5 0 014-2z\"/>","bookmark":"<path d=\"M17 3a2 2 0 0 1 2 2v15a1 1 0 0 1-1.496.868l-4.512-2.578a2 2 0 0 0-1.984 0l-4.512 2.578A1 1 0 0 1 5 20V5a2 2 0 0 1 2-2z\"/>","tag":"<path d=\"M12.586 2.586A2 2 0 0 0 11.172 2H4a2 2 0 0 0-2 2v7.172a2 2 0 0 0 .586 1.414l8.704 8.704a2.426 2.426 0 0 0 3.42 0l6.58-6.58a2.426 2.426 0 0 0 0-3.42z\"/><circle cx=\"7.5\" cy=\"7.5\" r=\".5\" fill=\"currentColor\"/>","gift":"<path d=\"M12 7v14\"/><path d=\"M20 11v8a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2v-8\"/><path d=\"M7.5 7a1 1 0 0 1 0-5A4.8 8 0 0 1 12 7a4.8 8 0 0 1 4.5-5 1 1 0 0 1 0 5\"/><rect x=\"3\" y=\"7\" width=\"18\" height=\"4\" rx=\"1\"/>","package":"<path d=\"M11 21.73a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73z\"/><path d=\"M12 22V12\"/><polyline points=\"3.29 7 12 12 20.71 7\"/><path d=\"m7.5 4.27 9 5.15\"/>","truck":"<path d=\"M14 18V6a2 2 0 0 0-2-2H4a2 2 0 0 0-2 2v11a1 1 0 0 0 1 1h2\"/><path d=\"M15 18H9\"/><path d=\"M19 18h2a1 1 0 0 0 1-1v-3.65a1 1 0 0 0-.22-.624l-3.48-4.35A1 1 0 0 0 17.52 8H14\"/><circle cx=\"17\" cy=\"18\" r=\"2\"/><circle cx=\"7\" cy=\"18\" r=\"2\"/>","car":"<path d=\"M19 17h2c.6 0 1-.4 1-1v-3c0-.9-.7-1.7-1.5-1.9C18.7 10.6 16 10 16 10s-1.3-1.4-2.2-2.3c-.5-.4-1.1-.7-1.8-.7H5c-.6 0-1.1.4-1.4.9l-1.4 2.9A3.7 3.7 0 0 0 2 12v4c0 .6.4 1 1 1h2\"/><circle cx=\"7\" cy=\"17\" r=\"2\"/><path d=\"M9 17h6\"/><circle cx=\"17\" cy=\"17\" r=\"2\"/>","bike":"<circle cx=\"18.5\" cy=\"17.5\" r=\"3.5\"/><circle cx=\"5.5\" cy=\"17.5\" r=\"3.5\"/><circle cx=\"15\" cy=\"5\" r=\"1\"/><path d=\"M12 17.5V14l-3-3 4-3 2 3h2\"/>","plane":"<path d=\"M17.8 19.2 16 11l3.5-3.5C21 6 21.5 4 21 3c-1-.5-3 0-4.5 1.5L13 8 4.8 6.2c-.5-.1-.9.1-1.1.5l-.3.5c-.2.5-.1 1 .3 1.3L9 12l-2 3H4l-1 1 3 2 2 3 1-1v-3l3-2 3.5 5.3c.3.4.8.5 1.3.3l.5-.2c.4-.3.6-.7.5-1.2z\"/>","building":"<path d=\"M10 12h4\"/><path d=\"M10 8h4\"/><path d=\"M14 21v-3a2 2 0 0 0-4 0v3\"/><path d=\"M6 10H4a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-2\"/><path d=\"M6 21V5a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v16\"/>","briefcase":"<path d=\"M16 20V4a2 2 0 0 0-2-2h-4a2 2 0 0 0-2 2v16\"/><rect width=\"20\" height=\"14\" x=\"2\" y=\"6\" rx=\"2\"/>","graduation-cap":"<path d=\"M21.42 10.922a1 1 0 0 0-.019-1.838L12.83 5.18a2 2 0 0 0-1.66 0L2.6 9.08a1 1 0 0 0 0 1.832l8.57 3.908a2 2 0 0 0 1.66 0z\"/><path d=\"M22 10v6\"/><path d=\"M6 12.5V16a6 3 0 0 0 12 0v-3.5\"/>","globe":"<circle cx=\"12\" cy=\"12\" r=\"10\"/><path d=\"M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20\"/><path d=\"M2 12h20\"/>","link":"<path d=\"M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71\"/><path d=\"M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71\"/>","qr-code":"<rect width=\"5\" height=\"5\" x=\"3\" y=\"3\" rx=\"1\"/><rect width=\"5\" height=\"5\" x=\"16\" y=\"3\" rx=\"1\"/><rect width=\"5\" height=\"5\" x=\"3\" y=\"16\" rx=\"1\"/><path d=\"M21 16h-3a2 2 0 0 0-2 2v3\"/><path d=\"M21 21v.01\"/><path d=\"M12 7v3a2 2 0 0 1-2 2H7\"/><path d=\"M3 12h.01\"/><path d=\"M12 3h.01\"/><path d=\"M12 16v.01\"/><path d=\"M16 12h1\"/><path d=\"M21 12v.01\"/><path d=\"M12 21v-1\"/>","scan-line":"<path d=\"M3 7V5a2 2 0 0 1 2-2h2\"/><path d=\"M17 3h2a2 2 0 0 1 2 2v2\"/><path d=\"M21 17v2a2 2 0 0 1-2 2h-2\"/><path d=\"M7 21H5a2 2 0 0 1-2-2v-2\"/><path d=\"M7 12h10\"/>","file-text":"<path d=\"M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z\"/><path d=\"M14 2v5a1 1 0 0 0 1 1h5\"/><path d=\"M10 9H8\"/><path d=\"M16 13H8\"/><path d=\"M16 17H8\"/>","folder":"<path d=\"M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z\"/>","refresh-cw":"<path d=\"M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8\"/><path d=\"M21 3v5h-5\"/><path d=\"M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16\"/><path d=\"M8 16H3v5\"/>","filter":"<path d=\"M10 20a1 1 0 0 0 .553.895l2 1A1 1 0 0 0 14 21v-7a2 2 0 0 1 .517-1.341L21.74 4.67A1 1 0 0 0 21 3H3a1 1 0 0 0-.742 1.67l7.225 7.989A2 2 0 0 1 10 14z\"/>","sliders-horizontal":"<path d=\"M10 5H3\"/><path d=\"M12 19H3\"/><path d=\"M14 3v4\"/><path d=\"M16 17v4\"/><path d=\"M21 12h-9\"/><path d=\"M21 19h-5\"/><path d=\"M21 5h-7\"/><path d=\"M8 10v4\"/><path d=\"M8 12H3\"/>","menu":"<path d=\"M4 5h16\"/><path d=\"M4 12h16\"/><path d=\"M4 19h16\"/>","ellipsis":"<circle cx=\"12\" cy=\"12\" r=\"1\"/><circle cx=\"19\" cy=\"12\" r=\"1\"/><circle cx=\"5\" cy=\"12\" r=\"1\"/>","chevron-right":"<path d=\"m9 18 6-6-6-6\"/>","chevron-left":"<path d=\"m15 18-6-6 6-6\"/>","arrow-right":"<path d=\"M5 12h14\"/><path d=\"m12 5 7 7-7 7\"/>","arrow-left":"<path d=\"m12 19-7-7 7-7\"/><path d=\"M19 12H5\"/>","log-out":"<path d=\"m16 17 5-5-5-5\"/><path d=\"M21 12H9\"/><path d=\"M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4\"/>","sparkles":"<path d=\"M11.017 2.814a1 1 0 0 1 1.966 0l1.051 5.558a2 2 0 0 0 1.594 1.594l5.558 1.051a1 1 0 0 1 0 1.966l-5.558 1.051a2 2 0 0 0-1.594 1.594l-1.051 5.558a1 1 0 0 1-1.966 0l-1.051-5.558a2 2 0 0 0-1.594-1.594l-5.558-1.051a1 1 0 0 1 0-1.966l5.558-1.051a2 2 0 0 0 1.594-1.594z\"/><path d=\"M20 2v4\"/><path d=\"M22 4h-4\"/><circle cx=\"4\" cy=\"20\" r=\"2\"/>","smile":"<path d=\"M15 10V9\"/><path d=\"M16.472 15a6 6 0 01-8.943 0\"/><path d=\"M9 10V9\"/><circle cx=\"12\" cy=\"12\" r=\"10\"/>","thumbs-up":"<path d=\"M15 5.88 14 10h5.83a2 2 0 0 1 1.92 2.56l-2.33 8A2 2 0 0 1 17.5 22H4a2 2 0 0 1-2-2v-8a2 2 0 0 1 2-2h2.76a2 2 0 0 0 1.79-1.11L12 2a3.13 3.13 0 0 1 3 3.88Z\"/><path d=\"M7 10v12\"/>","target":"<circle cx=\"12\" cy=\"12\" r=\"10\"/><circle cx=\"12\" cy=\"12\" r=\"6\"/><circle cx=\"12\" cy=\"12\" r=\"2\"/>","timer":"<line x1=\"10\" x2=\"14\" y1=\"2\" y2=\"2\"/><line x1=\"12\" x2=\"15\" y1=\"14\" y2=\"11\"/><circle cx=\"12\" cy=\"14\" r=\"8\"/>","receipt":"<path d=\"M12 17V7\"/><path d=\"M16 8h-6a2 2 0 0 0 0 4h4a2 2 0 0 1 0 4H8\"/><path d=\"M4 3a1 1 0 0 1 1-1 1.3 1.3 0 0 1 .7.2l.933.6a1.3 1.3 0 0 0 1.4 0l.934-.6a1.3 1.3 0 0 1 1.4 0l.933.6a1.3 1.3 0 0 0 1.4 0l.933-.6a1.3 1.3 0 0 1 1.4 0l.934.6a1.3 1.3 0 0 0 1.4 0l.933-.6A1.3 1.3 0 0 1 19 2a1 1 0 0 1 1 1v18a1 1 0 0 1-1 1 1.3 1.3 0 0 1-.7-.2l-.933-.6a1.3 1.3 0 0 0-1.4 0l-.934.6a1.3 1.3 0 0 1-1.4 0l-.933-.6a1.3 1.3 0 0 0-1.4 0l-.933.6a1.3 1.3 0 0 1-1.4 0l-.934-.6a1.3 1.3 0 0 0-1.4 0l-.933.6a1.3 1.3 0 0 1-.7.2 1 1 0 0 1-1-1z\"/>","percent":"<line x1=\"19\" x2=\"5\" y1=\"5\" y2=\"19\"/><circle cx=\"6.5\" cy=\"6.5\" r=\"2.5\"/><circle cx=\"17.5\" cy=\"17.5\" r=\"2.5\"/>","euro":"<path d=\"M4 10h12\"/><path d=\"M4 14h9\"/><path d=\"M19 6a7.7 7.7 0 0 0-5.2-2A7.9 7.9 0 0 0 6 12c0 4.4 3.5 8 7.8 8 2 0 3.8-.8 5.2-2\"/>","baby":"<path d=\"M10 16c.5.3 1.2.5 2 .5s1.5-.2 2-.5\"/><path d=\"M15 12h.01\"/><path d=\"M19.38 6.813A9 9 0 0 1 20.8 10.2a2 2 0 0 1 0 3.6 9 9 0 0 1-17.6 0 2 2 0 0 1 0-3.6A9 9 0 0 1 12 3c2 0 3.5 1.1 3.5 2.5s-.9 2.5-2 2.5c-.8 0-1.5-.4-1.5-1\"/><path d=\"M9 12h.01\"/>","paw-print":"<circle cx=\"11\" cy=\"4\" r=\"2\"/><circle cx=\"18\" cy=\"8\" r=\"2\"/><circle cx=\"20\" cy=\"16\" r=\"2\"/><path d=\"M9 10a5 5 0 0 1 5 5v3.5a3.5 3.5 0 0 1-6.84 1.045Q6.52 17.48 4.46 16.84A3.5 3.5 0 0 1 5.5 10Z\"/>","cake":"<path d=\"M20 21v-8a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8\"/><path d=\"M4 16s.5-1 2-1 2.5 2 4 2 2.5-2 4-2 2.5 2 4 2 2-1 2-1\"/><path d=\"M2 21h20\"/><path d=\"M7 8v3\"/><path d=\"M12 8v3\"/><path d=\"M17 8v3\"/><path d=\"M7 4h.01\"/><path d=\"M12 4h.01\"/><path d=\"M17 4h.01\"/>","shirt":"<path d=\"M20.38 3.46 16 2a4 4 0 0 1-8 0L3.62 3.46a2 2 0 0 0-1.34 2.23l.58 3.47a1 1 0 0 0 .99.84H6v10c0 1.1.9 2 2 2h8a2 2 0 0 0 2-2V10h2.15a1 1 0 0 0 .99-.84l.58-3.47a2 2 0 0 0-1.34-2.23z\"/>","scissors":"<circle cx=\"6\" cy=\"6\" r=\"3\"/><path d=\"M8.12 8.12 12 12\"/><path d=\"M20 4 8.12 15.88\"/><circle cx=\"6\" cy=\"18\" r=\"3\"/><path d=\"M14.8 14.8 20 20\"/>","wrench":"<path d=\"M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.106-3.105c.32-.322.863-.22.983.218a6 6 0 0 1-8.259 7.057l-7.91 7.91a1 1 0 0 1-2.999-3l7.91-7.91a6 6 0 0 1 7.057-8.259c.438.12.54.662.219.984z\"/>","palette":"<path d=\"M12 22a1 1 0 0 1 0-20 10 9 0 0 1 10 9 5 5 0 0 1-5 5h-2.25a1.75 1.75 0 0 0-1.4 2.8l.3.4a1.75 1.75 0 0 1-1.4 2.8z\"/><circle cx=\"13.5\" cy=\"6.5\" r=\".5\" fill=\"currentColor\"/><circle cx=\"17.5\" cy=\"10.5\" r=\".5\" fill=\"currentColor\"/><circle cx=\"6.5\" cy=\"12.5\" r=\".5\" fill=\"currentColor\"/><circle cx=\"8.5\" cy=\"7.5\" r=\".5\" fill=\"currentColor\"/>"};
  // Ce qu'une retouche peut changer d'un élément.
  const STYLE_KEYS = [
    "width", "height", "min-height", "max-width", "padding", "margin", "gap", "border-radius", "background", "color",
    "border", "box-shadow", "opacity", "font-size", "font-weight", "font-family", "font-style", "letter-spacing",
    "line-height", "text-align", "text-transform", "display", "flex-direction", "align-items", "justify-content",
    "order", "flex", "align-self", "grid-template-columns", "grid-column", "grid-row", "min-width",
  ];
  const FONTS = {
    "Inter": "Inter:wght@400;500;600;700", "Manrope": "Manrope:wght@400;500;600;700",
    "DM Sans": "DM+Sans:wght@400;500;600;700", "Poppins": "Poppins:wght@400;500;600;700",
    "Space Grotesk": "Space+Grotesk:wght@400;500;600;700", "Nunito": "Nunito:wght@400;600;700;800",
    "Playfair Display": "Playfair+Display:wght@400;600;700", "Newsreader": "Newsreader:wght@400;500;600",
    "Fraunces": "Fraunces:wght@400;600;700", "JetBrains Mono": "JetBrains+Mono:wght@400;600",
  };

  const empty = () => ({ version: VERSION, tokens: {}, fonts: [], items: {}, added: [], screens: [] });
  const normalize = (value) => {
    const next = { ...empty(), ...(value && typeof value === "object" ? value : {}) };
    for (const key of ["tokens", "items"]) if (!next[key] || typeof next[key] !== "object" || Array.isArray(next[key])) next[key] = {};
    for (const key of ["fonts", "added", "screens"]) if (!Array.isArray(next[key])) next[key] = [];
    return next;
  };
  let design = empty();
  let host = null;
  // L'éditeur a déjà posé ses retouches : le fichier lu ensuite ne les écrase pas.
  let touched = false;

  // --- Clés : un élément est retrouvé par son ancre (id, écran, widget ajouté) et son chemin ---
  const own = (el) => el.nodeType === 1 && el.hasAttribute(OWN) && !el.hasAttribute("data-claw-id");
  const usableId = (id) => /^[A-Za-z][\w-]*$/.test(id || "");
  const anchorOf = (el) => {
    for (let node = el; node && node !== document.body && node !== document.documentElement; node = node.parentElement) {
      if (node.hasAttribute("data-claw-id")) return [node, `[data-claw-id="${node.getAttribute("data-claw-id")}"]`];
      if (usableId(node.id)) return [node, `#${node.id}`];
      if (node.dataset && node.dataset.screen) return [node, `[data-screen="${node.dataset.screen}"]`];
      if (node.hasAttribute("data-claw-screen")) return [node, `[data-claw-screen="${node.getAttribute("data-claw-screen")}"]`];
    }
    return [document.body, "body"];
  };
  const siblingsOf = (parent, tag) => [...parent.children].filter((child) => child.tagName.toLowerCase() === tag && !own(child));
  const keyOf = (el) => {
    if (!el || el === document.body || el === document.documentElement) return "body";
    const [anchor, selector] = anchorOf(el);
    const steps = [];
    for (let node = el; node && node !== anchor; node = node.parentElement) {
      const tag = node.tagName.toLowerCase();
      steps.unshift(`${tag}:${siblingsOf(node.parentElement, tag).indexOf(node)}`);
    }
    return steps.length ? `${selector}|${steps.join("/")}` : selector;
  };
  const find = (key) => {
    if (!key) return null;
    if (key === "body") return document.body;
    const [selector, path] = key.split("|");
    let node;
    try {
      node = selector === "body" ? document.body : document.querySelector(selector);
    } catch (_) {
      return null;
    }
    if (!node || !path) return node;
    for (const step of path.split("/")) {
      const [tag, index] = step.split(":");
      node = siblingsOf(node, tag)[Number(index)];
      if (!node) return null;
    }
    return node;
  };

  // --- Widgets ajoutés à la main ---------------------------------------------------------------
  const icon = (name, size = 20) => (ICONS[name]
    ? `<svg ${OWN} xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${ICONS[name]}</svg>`
    : "");
  const esc = (text) => String(text == null ? "" : text).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
  const WIDGETS = {
    title: { label: "Titre", props: { text: "Un titre" }, html: (p) => `<h2 class="claw-w-title">${esc(p.text)}</h2>` },
    text: { label: "Texte", props: { text: "Un paragraphe de texte à modifier." }, html: (p) => `<p class="claw-w-text">${esc(p.text)}</p>` },
    button: { label: "Bouton", props: { text: "Bouton", icon: "" }, html: (p) => `<button type="button" class="claw-w-button">${icon(p.icon)}<span>${esc(p.text)}</span></button>` },
    "button-soft": { label: "Bouton discret", props: { text: "Bouton", icon: "" }, html: (p) => `<button type="button" class="claw-w-button claw-w-button--soft">${icon(p.icon)}<span>${esc(p.text)}</span></button>` },
    card: { label: "Carte", props: { title: "Titre de la carte", text: "Quelques mots sur ce qu'elle contient." }, html: (p) => `<div class="claw-w-card"><h3>${esc(p.title)}</h3><p>${esc(p.text)}</p></div>` },
    stat: { label: "Chiffre clé", props: { value: "1 234 €", label: "Solde du mois" }, html: (p) => `<div class="claw-w-stat"><span>${esc(p.label)}</span><strong>${esc(p.value)}</strong></div>` },
    row: { label: "Ligne de liste", props: { icon: "shopping-cart", title: "Un élément", sub: "Un détail", value: "12 €" }, html: (p) => `<div class="claw-w-row"><span class="claw-w-row__icon">${icon(p.icon)}</span><span class="claw-w-row__main"><strong>${esc(p.title)}</strong><small>${esc(p.sub)}</small></span><b>${esc(p.value)}</b></div>` },
    badge: { label: "Pastille", props: { text: "Nouveau" }, html: (p) => `<span class="claw-w-badge">${esc(p.text)}</span>` },
    progress: { label: "Jauge", props: { label: "Objectif", value: 60 }, html: (p) => `<div class="claw-w-progress"><span>${esc(p.label)}</span><div><i style="width:${Math.max(0, Math.min(100, Number(p.value) || 0))}%"></i></div></div>` },
    image: { label: "Image", props: { src: "", alt: "" }, html: (p) => (p.src ? `<img class="claw-w-image" src="${esc(p.src)}" alt="${esc(p.alt)}">` : `<div class="claw-w-image claw-w-image--empty">${icon("image", 28)}<span>Image</span></div>`) },
    field: { label: "Champ", props: { label: "Nom", placeholder: "Votre nom" }, html: (p, id) => `<label class="claw-w-field" for="${id}-in"><span>${esc(p.label)}</span><input id="${id}-in" placeholder="${esc(p.placeholder)}"></label>` },
    icon: { label: "Icône", props: { icon: "star" }, html: (p) => `<span class="claw-w-icon">${icon(p.icon, 28)}</span>` },
    divider: { label: "Séparateur", props: {}, html: () => `<hr class="claw-w-divider">` },
    spacer: { label: "Espace", props: {}, html: () => `<div class="claw-w-spacer"></div>` },
    group: { label: "Groupe (rangée)", props: {}, html: () => `<div class="claw-w-group"></div>` },
  };
  const WIDGET_CSS = `
.claw-w-title{margin:.4em 0 .2em;font:700 1.5rem/1.2 var(--font-display,inherit);color:var(--ink,inherit)}
.claw-w-text{margin:.3em 0;color:var(--muted,inherit);line-height:1.55}
.claw-w-button{display:inline-flex;align-items:center;justify-content:center;gap:8px;min-height:46px;padding:0 20px;border:0;border-radius:var(--radius-sm,14px);background:var(--accent,#5b4bd6);color:var(--on-accent,#fff);font-family:inherit;font-size:1rem;font-weight:600;line-height:1;cursor:pointer}
.claw-w-button--soft{background:var(--accent-soft,rgba(91,75,214,.12));color:var(--accent-ink,var(--accent,#5b4bd6))}
.claw-w-card{padding:18px;border-radius:var(--radius,20px);background:var(--surface,#fff);color:var(--ink,inherit);box-shadow:var(--shadow,0 6px 20px rgba(0,0,0,.06));border:1px solid var(--line,rgba(0,0,0,.08))}
.claw-w-card h3{margin:0 0 6px;font:600 1.05rem/1.3 var(--font-display,inherit)}.claw-w-card p{margin:0;color:var(--muted,inherit)}
.claw-w-stat{display:grid;gap:4px;padding:18px;border-radius:var(--radius,20px);background:var(--accent,#5b4bd6);color:var(--on-accent,#fff)}
.claw-w-stat span{opacity:.85;font-size:.9rem}.claw-w-stat strong{font:700 2rem/1.1 var(--font-display,inherit);font-variant-numeric:tabular-nums}
.claw-w-row{display:flex;align-items:center;gap:12px;min-height:60px;padding:8px 4px;border-bottom:1px solid var(--line,rgba(0,0,0,.08))}
.claw-w-row__icon{display:grid;place-items:center;width:40px;height:40px;border-radius:12px;background:var(--accent-soft,rgba(91,75,214,.12));color:var(--accent,#5b4bd6)}
.claw-w-row__main{flex:1;min-width:0;display:grid}.claw-w-row__main small{color:var(--muted,inherit)}
.claw-w-badge{display:inline-flex;align-items:center;min-height:26px;padding:0 10px;border-radius:999px;background:var(--accent-soft,rgba(91,75,214,.12));color:var(--accent-ink,var(--accent,#5b4bd6));font-size:.8rem;font-weight:600}
.claw-w-progress{display:grid;gap:8px}.claw-w-progress span{font-weight:600}
.claw-w-progress div{height:10px;border-radius:99px;background:var(--surface-2,rgba(0,0,0,.07));overflow:hidden}.claw-w-progress i{display:block;height:100%;border-radius:inherit;background:var(--accent,#5b4bd6)}
.claw-w-image{display:block;width:100%;max-height:320px;object-fit:cover;border-radius:var(--radius,20px)}
.claw-w-image--empty{display:grid;place-items:center;gap:6px;height:160px;background:var(--surface-2,rgba(0,0,0,.05));color:var(--muted,#777)}
.claw-w-field{display:grid;gap:6px;font-weight:600}.claw-w-field input{min-height:46px;padding:0 14px;border:1px solid var(--line,rgba(0,0,0,.15));border-radius:var(--radius-sm,14px);background:var(--field,#fff);color:inherit;font:inherit;font-weight:400}
.claw-w-icon{display:inline-grid;place-items:center;color:var(--accent,#5b4bd6)}
.claw-w-divider{border:0;border-top:1px solid var(--line,rgba(0,0,0,.1));margin:12px 0}
.claw-w-spacer{height:24px}
.claw-w-group{display:flex;flex-wrap:wrap;gap:10px;align-items:center;min-height:24px}
[data-claw-root]{display:flex;flex-direction:column;gap:14px;max-width:640px;margin:0 auto;padding:20px 16px 110px}
[data-claw-screen]{display:flex;flex-direction:column;gap:14px}
[data-claw-screen][hidden]{display:none}
.claw-tabbar{position:fixed;left:12px;right:12px;bottom:calc(12px + env(safe-area-inset-bottom));display:flex;justify-content:space-around;max-width:520px;margin:0 auto;padding:6px;border-radius:22px;background:var(--surface,#fff);box-shadow:var(--shadow,0 6px 24px rgba(0,0,0,.12));border:1px solid var(--line,rgba(0,0,0,.08));z-index:20}
.claw-tabbar a{flex:1;display:grid;justify-items:center;gap:2px;padding:6px 4px;border-radius:16px;color:var(--muted,#666);text-decoration:none;font-size:.75rem;font-weight:600}
.claw-tabbar a[aria-current]{color:var(--accent,#5b4bd6);background:var(--accent-soft,rgba(91,75,214,.1))}
`;

  // --- Application ---------------------------------------------------------------------------
  const applied = new WeakMap();
  const firstText = (el) => {
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT, {
      acceptNode: (node) => (node.data.trim() && !node.parentElement.closest("svg, style, script") ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT),
    });
    return walker.nextNode();
  };
  const setStyles = (el, style) => {
    const before = applied.get(el) || new Set();
    const now = new Set();
    for (const [prop, value] of Object.entries(style || {})) {
      if (!STYLE_KEYS.includes(prop) || value === "" || value == null) continue;
      now.add(prop);
      if (el.style.getPropertyValue(prop) !== String(value) || el.style.getPropertyPriority(prop) !== "important") {
        el.style.setProperty(prop, String(value), "important");
      }
    }
    for (const prop of before) if (!now.has(prop)) el.style.removeProperty(prop);
    applied.set(el, now);
  };
  const setIcon = (el, name) => {
    const ours = el.querySelector("svg[data-claw-icon]");
    const theirs = [...el.querySelectorAll("svg")].find((svg) => !svg.hasAttribute("data-claw-icon"));
    if (!name || !ICONS[name]) {
      if (ours) ours.remove();
      if (theirs && theirs.dataset.clawHid) { theirs.style.removeProperty("display"); delete theirs.dataset.clawHid; }
      return;
    }
    if (ours && ours.getAttribute("data-claw-icon") === name) return;
    if (ours) ours.remove();
    const size = theirs ? Math.round(theirs.getBoundingClientRect().width) || 20 : 20;
    const wrap = document.createElement("span");
    wrap.innerHTML = icon(name, size);
    const svg = wrap.firstElementChild;
    svg.setAttribute("data-claw-icon", name);
    if (theirs) {
      svg.setAttribute("class", theirs.getAttribute("class") || "");
      theirs.parentNode.insertBefore(svg, theirs);
      theirs.style.setProperty("display", "none", "important");
      theirs.dataset.clawHid = "1";
    } else {
      el.insertBefore(svg, el.firstChild);
    }
  };
  // Grille : un bloc dont les enfants se placent sur des colonnes (`grid`), et la place de chaque
  // enfant (`place` : colonne de départ, largeur en colonnes, ligne). Un enfant sans place prend
  // toute la largeur.
  let gridParents = new Set();
  const gridStyle = (el, entry) => {
    const style = {};
    if (entry && entry.grid) {
      const columns = Math.max(1, Math.min(12, Number(entry.grid.columns) || 4));
      Object.assign(style, { display: "grid", "grid-template-columns": `repeat(${columns}, minmax(0, 1fr))`, gap: `${Number(entry.grid.gap ?? 12)}px`, "align-items": "start" });
    }
    if (el.parentElement && gridParents.has(el.parentElement)) {
      const place = entry && entry.place;
      Object.assign(style, place
        ? { "grid-column": `${place.col} / span ${place.span}`, "grid-row": String(place.row), "min-width": "0" }
        : { "grid-column": "1 / -1", "min-width": "0" });
    }
    return style;
  };
  const applyItem = (el, item) => {
    const style = { ...gridStyle(el, item), ...(item.style || {}) };
    if (item.hidden && !(host && host.showHidden)) style.display = "none";
    if (item.hidden && host && host.showHidden) style.opacity = "0.35";
    setStyles(el, style);
    if (typeof item.text === "string") {
      const node = firstText(el);
      if (node && node.data !== item.text) node.data = item.text;
    }
    if (item.icon !== undefined) setIcon(el, item.icon);
  };
  const rootOf = (widget) => {
    if (widget.in && widget.in.startsWith("screen:")) return document.querySelector(`[data-claw-screen="${widget.in.slice(7)}"]`);
    if (widget.in) return find(widget.in);
    return document.querySelector("[data-claw-root]") || document.querySelector("main") || document.querySelector("#root > *") || document.body;
  };
  const renderWidget = (widget) => {
    const kind = WIDGETS[widget.type];
    if (!kind) return null;
    const signature = JSON.stringify([widget.type, widget.props]);
    let el = document.querySelector(`[data-claw-id="${widget.id}"]`);
    if (el && el.__clawSignature === signature) return el;
    const wrap = document.createElement("div");
    wrap.innerHTML = kind.html({ ...kind.props, ...(widget.props || {}) }, widget.id).trim();
    const fresh = wrap.firstElementChild;
    fresh.setAttribute(OWN, "");
    fresh.setAttribute("data-claw-id", widget.id);
    fresh.__clawSignature = signature;
    if (el) el.replaceWith(fresh);
    return fresh;
  };
  const placeWidgets = () => {
    const wanted = new Set(design.added.map((widget) => widget.id));
    for (const el of document.querySelectorAll("[data-claw-id]")) if (!wanted.has(el.getAttribute("data-claw-id"))) el.remove();
    for (const widget of design.added) {
      const parent = rootOf(widget);
      if (!parent) continue;
      const el = renderWidget(widget);
      if (!el) continue;
      const after = widget.after ? find(widget.after) : null;
      if (after && after.parentElement === parent) {
        if (after.nextSibling !== el) parent.insertBefore(el, after.nextSibling);
      } else if (el.parentElement !== parent) {
        parent.appendChild(el);
      }
    }
  };
  const styleWidgets = () => {
    for (const widget of design.added) {
      const el = document.querySelector(`[data-claw-id="${widget.id}"]`);
      if (!el) continue;
      const style = { ...gridStyle(el, widget), ...(widget.style || {}) };
      setStyles(el, widget.hidden ? { ...style, display: "none" } : style);
    }
  };
  // Appli construite à la main : ses écrans et sa barre d'onglets.
  const placeScreens = () => {
    const root = document.querySelector("[data-claw-root]");
    if (!root) return;
    const screens = design.screens.length ? design.screens : [{ id: "accueil", label: "Accueil", icon: "house" }];
    for (const screen of screens) {
      if (!root.querySelector(`:scope > [data-claw-screen="${screen.id}"]`)) {
        const section = document.createElement("section");
        section.setAttribute(OWN, "");
        section.setAttribute("data-claw-screen", screen.id);
        root.appendChild(section);
      }
    }
    for (const section of root.querySelectorAll(":scope > [data-claw-screen]")) {
      if (!screens.some((screen) => screen.id === section.getAttribute("data-claw-screen"))) section.remove();
    }
    const current = screens.find((screen) => `#${screen.id}` === location.hash) || screens[0];
    for (const section of root.querySelectorAll(":scope > [data-claw-screen]")) section.hidden = section.getAttribute("data-claw-screen") !== current.id;
    let bar = document.querySelector(".claw-tabbar");
    if (screens.length < 2) {
      if (bar) bar.remove();
      return;
    }
    if (!bar) {
      bar = document.createElement("nav");
      bar.className = "claw-tabbar";
      bar.setAttribute(OWN, "");
      bar.setAttribute("aria-label", "Écrans");
      document.body.appendChild(bar);
    }
    const html = screens.map((screen) => `<a href="#${esc(screen.id)}"${screen.id === current.id ? ' aria-current="page"' : ""}>${icon(screen.icon || "circle", 22)}<span>${esc(screen.label)}</span></a>`).join("");
    if (bar.__clawHtml !== html) {
      bar.innerHTML = html;
      bar.__clawHtml = html;
    }
  };
  const ensureStyle = (id, css) => {
    let el = document.getElementById(id);
    if (!el) {
      el = document.createElement("style");
      el.id = id;
      el.setAttribute(OWN, "");
      document.head.appendChild(el);
    }
    if (el.textContent !== css) el.textContent = css;
  };
  const applyTokens = () => {
    const tokens = Object.entries(design.tokens).filter(([name, value]) => /^--[\w-]+$/.test(name) && value);
    ensureStyle("claw-design-tokens", tokens.length ? `:root,:root[data-theme],html body{${tokens.map(([name, value]) => `${name}:${String(value).replace(/[;{}]/g, "")} !important`).join(";")}}` : "");
    for (const name of design.fonts) {
      if (!FONTS[name] || document.querySelector(`link[data-claw-font="${name}"]`)) continue;
      const link = document.createElement("link");
      link.rel = "stylesheet";
      link.href = `https://fonts.googleapis.com/css2?family=${FONTS[name]}&display=swap`;
      link.setAttribute(OWN, "");
      link.setAttribute("data-claw-font", name);
      document.head.appendChild(link);
    }
  };
  let busy = false;
  const apply = () => {
    if (busy || !document.body) return;
    busy = true;
    try {
      ensureStyle("claw-design-widgets", WIDGET_CSS);
      applyTokens();
      placeScreens();
      placeWidgets();
      gridParents = new Set();
      for (const [key, item] of Object.entries(design.items)) {
        if (item && item.grid) { const el = find(key); if (el) gridParents.add(el); }
      }
      for (const widget of design.added) {
        if (widget.grid) { const el = document.querySelector(`[data-claw-id="${widget.id}"]`); if (el) gridParents.add(el); }
      }
      styleWidgets();
      const done = new Set();
      for (const [key, item] of Object.entries(design.items)) {
        const el = find(key);
        if (el && item && typeof item === "object") { applyItem(el, item); done.add(el); }
      }
      // Les enfants d'une grille que rien ne règle : toute la largeur.
      for (const parent of gridParents) {
        for (const child of parent.children) {
          if (!own(child) && !child.hasAttribute("data-claw-id") && !done.has(child)) setStyles(child, gridStyle(child, null));
        }
      }
      if (host) frame();
    } finally {
      busy = false;
    }
  };
  // L'appli redessine ses écrans : on repasse après elle, une fois par image.
  let queued = false;
  const later = () => {
    if (queued) return;
    queued = true;
    requestAnimationFrame(() => {
      queued = false;
      apply();
    });
  };
  const observer = new MutationObserver((records) => {
    if (records.some((record) => !(record.target.nodeType === 1 && record.target.closest && record.target.closest(`[${OWN}]:not([data-claw-id])`)))) later();
  });
  const start = () => {
    observer.observe(document.body, { subtree: true, childList: true, characterData: true, attributes: true, attributeFilter: ["open"] });
    addEventListener("hashchange", later);
    apply();
  };

  // --- Réordonner : l'ordre visuel des enfants d'un parent passe par `order` ----------------------
  const childrenOf = (parent) => [...parent.children].filter((child) => !own(child) && !["SCRIPT", "STYLE", "TEMPLATE"].includes(child.tagName)
    && getComputedStyle(child).display !== "none" && getComputedStyle(child).position !== "fixed");
  const visualOrder = (parent) => childrenOf(parent)
    .map((child, index) => ({ child, index, order: Number(getComputedStyle(child).order) || 0 }))
    .sort((a, b) => a.order - b.order || a.index - b.index)
    .map((entry) => entry.child);
  const itemOf = (target, key) => (target.items[key] = target.items[key] || {});
  const styleOf = (target, el) => {
    const id = el.getAttribute("data-claw-id");
    if (id) {
      const widget = target.added.find((entry) => entry.id === id);
      if (widget) return (widget.style = widget.style || {});
    }
    const item = itemOf(target, keyOf(el));
    return (item.style = item.style || {});
  };
  /** Place l'élément `key` au rang `to` parmi les enfants visibles de son parent (mutation de `target`). */
  const reorder = (target, key, to) => {
    const el = find(key);
    if (!el || !el.parentElement) return target;
    const parent = el.parentElement;
    const list = visualOrder(parent).filter((child) => child !== el);
    list.splice(Math.max(0, Math.min(list.length, to)), 0, el);
    const display = getComputedStyle(parent).display;
    if (!/flex|grid/.test(display)) {
      const style = styleOf(target, parent);
      style.display = "flex";
      style["flex-direction"] = "column";
    }
    list.forEach((child, index) => { styleOf(target, child).order = String(index + 1); });
    return target;
  };
  const indexIn = (el) => visualOrder(el.parentElement).indexOf(el);
  /** Ce qui règle un élément dans `target` : son widget ajouté, ou son entrée (créée au besoin). */
  const entryOf = (target, el) => {
    const id = el.getAttribute("data-claw-id");
    const widget = id ? target.added.find((entry) => entry.id === id) : null;
    return widget || itemOf(target, keyOf(el));
  };
  const peek = (el) => {
    const id = el.getAttribute("data-claw-id");
    return (id ? design.added.find((entry) => entry.id === id) : design.items[keyOf(el)]) || null;
  };
  /** Les enfants du bloc `key` passent sur une grille de `columns` colonnes, chacun sur sa ligne. */
  const enableGrid = (target, key, columns = 4, gap = 12) => {
    const el = find(key);
    if (!el) return target;
    const entry = entryOf(target, el);
    entry.grid = { columns, gap };
    if (entry.style) { delete entry.style.display; delete entry.style["flex-direction"]; delete entry.style["flex-wrap"]; }
    visualOrder(el).forEach((child, index) => {
      const kid = entryOf(target, child);
      kid.place = { col: 1, span: columns, row: index + 1 };
      if (kid.style) { delete kid.style.order; delete kid.style.width; }
    });
    return target;
  };
  const disableGrid = (target, key) => {
    const el = find(key);
    if (!el) return target;
    delete entryOf(target, el).grid;
    for (const child of childrenOf(el)) delete entryOf(target, child).place;
    return target;
  };
  const setGrid = (target, key, { columns, gap }) => {
    const el = find(key);
    if (!el) return target;
    const entry = entryOf(target, el);
    entry.grid = { columns: columns ?? entry.grid.columns, gap: gap ?? entry.grid.gap };
    const n = entry.grid.columns;
    for (const child of childrenOf(el)) {
      const kid = entryOf(target, child);
      if (!kid.place) continue;
      kid.place.span = Math.min(kid.place.span, n);
      kid.place.col = Math.min(kid.place.col, n - kid.place.span + 1);
    }
    return target;
  };
  /** Place `key` sur la grille de son parent : colonne, largeur (span), ligne ; `insert` ouvre une
   * ligne neuve à cet endroit. Un bloc déjà là descend d'une ligne ; les lignes vides se referment. */
  const placeAt = (target, key, spec) => {
    const el = find(key);
    if (!el || !el.parentElement) return target;
    const parent = el.parentElement;
    const grid = entryOf(target, parent).grid;
    if (!grid) return target;
    const n = grid.columns;
    const kids = childrenOf(parent).map((child) => ({ child, entry: entryOf(target, child) }));
    let last = Math.max(0, ...kids.filter((kid) => kid.entry.place).map((kid) => kid.entry.place.row));
    for (const kid of kids) if (!kid.entry.place) kid.entry.place = { col: 1, span: n, row: (last += 1) };
    const mine = kids.find((kid) => kid.child === el).entry;
    const span = Math.max(1, Math.min(n, Math.round(spec.span ?? mine.place.span)));
    const col = Math.max(1, Math.min(n - span + 1, Math.round(spec.col ?? mine.place.col)));
    const row = Math.max(1, Math.round(spec.row ?? mine.place.row));
    if (spec.insert) for (const kid of kids) if (kid.entry !== mine && kid.entry.place.row >= row) kid.entry.place.row += 1;
    mine.place = { col, span, row };
    const clash = kids.filter((kid) => kid.entry !== mine && kid.entry.place.row === row
      && kid.entry.place.col < col + span && col < kid.entry.place.col + kid.entry.place.span);
    if (clash.length) {
      for (const kid of kids) if (kid.entry !== mine && kid.entry.place.row > row) kid.entry.place.row += 1;
      for (const kid of clash) kid.entry.place.row = row + 1;
    }
    const rows = [...new Set(kids.map((kid) => kid.entry.place.row))].sort((a, b) => a - b);
    for (const kid of kids) kid.entry.place.row = rows.indexOf(kid.entry.place.row) + 1;
    if (spec.height) {
      mine.style = mine.style || {};
      mine.style.height = spec.height;
    }
    return target;
  };
  const gridMetrics = (parent) => {
    const entry = peek(parent);
    if (!entry || !entry.grid) return null;
    const rect = parent.getBoundingClientRect();
    const style = getComputedStyle(parent);
    const n = entry.grid.columns;
    const gap = parseFloat(style.columnGap) || 0;
    const left = rect.left + (parseFloat(style.paddingLeft) || 0);
    const width = rect.width - (parseFloat(style.paddingLeft) || 0) - (parseFloat(style.paddingRight) || 0);
    return { n, gap, left, width, rect, col: (width - gap * (n - 1)) / n };
  };

  // --- Éditeur ---------------------------------------------------------------------------------
  let selected = null;
  let box = null;
  const EDITOR_CSS = `
.claw-edit-box,.claw-edit-hover,.claw-edit-drop,.claw-edit-grid{inset:auto;margin:0;padding:0;overflow:visible;color:inherit;background:transparent;border:0}
.claw-edit-box[hidden],.claw-edit-hover[hidden],.claw-edit-drop[hidden],.claw-edit-grid[hidden]{display:none!important}
.claw-edit-box{position:fixed;z-index:2147483646;pointer-events:none;border:2px solid #2f80ed;border-radius:6px;box-shadow:0 0 0 9999px rgba(15,23,42,.06);transition:all .08s}
.claw-edit-box b{position:absolute;left:-2px;top:-24px;max-width:240px;overflow:hidden;white-space:nowrap;text-overflow:ellipsis;padding:2px 8px;border-radius:6px 6px 6px 0;background:#2f80ed;color:#fff;font:600 12px/18px system-ui,sans-serif}
.claw-edit-box i{position:absolute;right:-9px;bottom:-9px;width:18px;height:18px;border-radius:50%;background:#fff;border:2px solid #2f80ed;pointer-events:auto;cursor:nwse-resize;touch-action:none}
.claw-edit-hover{position:fixed;z-index:2147483645;pointer-events:none;border:1px dashed #2f80ed;border-radius:6px}
.claw-edit-drop{position:fixed;z-index:2147483647;pointer-events:none;height:3px;border-radius:3px;background:#2f80ed}
.claw-edit-box u{position:absolute;right:-2px;top:-34px;display:grid;place-items:center;width:30px;height:30px;border-radius:50%;background:#2f80ed;color:#fff;pointer-events:auto;cursor:grab;touch-action:none;box-shadow:0 2px 6px rgba(0,0,0,.25)}
.claw-edit-box u svg{width:18px;height:18px}
.claw-edit-box em{position:absolute;left:-2px;bottom:-22px;padding:1px 6px;border-radius:0 0 6px 6px;background:#2f80ed;color:#fff;font:600 11px/18px system-ui,sans-serif;font-style:normal;white-space:nowrap}
.claw-edit-grid{position:fixed;z-index:2147483644;pointer-events:none;display:grid}
.claw-edit-grid span{position:relative;background:rgba(47,128,237,.08);border-left:1px dashed rgba(47,128,237,.55);border-right:1px dashed rgba(47,128,237,.55)}
.claw-edit-grid span b{position:absolute;left:50%;top:-18px;transform:translateX(-50%);padding:0 4px;border-radius:4px;background:rgba(47,128,237,.9);color:#fff;font:600 10px/15px system-ui,sans-serif;white-space:nowrap}
.claw-edit-grid > s{position:absolute;left:0;right:0;border-top:1px dashed rgba(47,128,237,.75);border-bottom:1px dashed rgba(47,128,237,.75);background:rgba(47,128,237,.05);text-decoration:none}
.claw-edit-grid > s b{position:absolute;right:4px;bottom:2px;padding:0 4px;border-radius:4px;background:rgba(47,128,237,.85);color:#fff;font:600 9px/14px system-ui,sans-serif;white-space:nowrap}
.claw-edit-grid > i{position:absolute;right:0;bottom:-20px;padding:0 6px;border-radius:4px;background:rgba(47,128,237,.9);color:#fff;font:600 10px/16px system-ui,sans-serif;font-style:normal;white-space:nowrap}
.claw-edit-drop.cell{height:auto;background:rgba(47,128,237,.18);border:2px dashed #2f80ed;border-radius:8px}
html.claw-editing,html.claw-editing *{cursor:default;-webkit-user-select:none;user-select:none}`;
  const NAMES = { button: "Bouton", a: "Lien", h1: "Titre", h2: "Titre", h3: "Sous-titre", h4: "Sous-titre", p: "Texte", span: "Texte", strong: "Texte", small: "Petit texte", img: "Image", svg: "Icône", input: "Champ", label: "Libellé", section: "Section", nav: "Barre", header: "En-tête", footer: "Pied", ul: "Liste", li: "Élément de liste", main: "Contenu", form: "Formulaire", hr: "Séparateur" };
  const nameOf = (el) => {
    const id = el.getAttribute("data-claw-id");
    if (id) {
      const widget = design.added.find((entry) => entry.id === id);
      if (widget && WIDGETS[widget.type]) return WIDGETS[widget.type].label;
    }
    const tag = el.tagName.toLowerCase();
    const text = (el.innerText || el.getAttribute("aria-label") || "").replace(/\s+/g, " ").trim();
    const base = NAMES[tag] || (/card|carte/i.test(el.className) ? "Carte" : "Bloc");
    return text && text.length < 40 ? `${base} « ${text} »` : base;
  };
  const pick = (target) => {
    let el = target.closest("svg") ? target.closest("svg").parentElement || target : target;
    if (el.closest(`.claw-edit-box, .claw-edit-hover`)) return null;
    // Un texte nu dans un bouton : le bouton est l'élément qui compte.
    const control = el.closest("button, a, [role=button], label, input, select, textarea");
    if (control && control !== el && control.contains(el)) el = control;
    // Le premier toucher prend le bloc entier (case d'une grille, widget ajouté) ; un second
    // toucher dans le bloc déjà choisi va dedans.
    for (let node = el; node && node !== document.body; node = node.parentElement) {
      if (node.hasAttribute("data-claw-id") || (node.parentElement && gridParents.has(node.parentElement))) {
        if (node !== el && !(selected && (selected === node || node.contains(selected)))) el = node;
        break;
      }
    }
    return el === document.documentElement ? document.body : el;
  };
  const info = (el) => {
    const style = getComputedStyle(el);
    const key = keyOf(el);
    const id = el.getAttribute("data-claw-id");
    const widget = id ? design.added.find((entry) => entry.id === id) : null;
    const text = firstText(el);
    const parent = el.parentElement && el.parentElement !== document.documentElement ? el.parentElement : null;
    return {
      key, name: nameOf(el), tag: el.tagName.toLowerCase(), widget: widget ? JSON.parse(JSON.stringify(widget)) : null,
      text: text ? text.data.trim() : null, hasIcon: Boolean(el.querySelector("svg") || el.matches("svg")),
      container: el.children.length > 0 || /^(div|section|main|ul|ol|nav|header|footer|form|article|aside)$/i.test(el.tagName),
      parentKey: parent ? keyOf(parent) : null, index: parent ? indexIn(el) : 0, count: parent ? visualOrder(parent).length : 1,
      item: JSON.parse(JSON.stringify(design.items[key] || {})),
      grid: peek(el) && peek(el).grid ? { ...peek(el).grid } : null,
      place: peek(el) && peek(el).place ? { ...peek(el).place } : null,
      parentGrid: parent && peek(parent) && peek(parent).grid ? { ...peek(parent).grid } : null,
      size: [Math.round(el.getBoundingClientRect().width), Math.round(el.getBoundingClientRect().height)],
      computed: Object.fromEntries(["color", "background-color", "font-size", "font-weight", "font-family", "text-align", "width", "height", "padding-top", "padding-left", "margin-top", "margin-bottom", "border-radius", "display", "opacity"].map((prop) => [prop, style.getPropertyValue(prop)])),
    };
  };
  // Les fenêtres (<dialog> ouvert par showModal) passent au-dessus de toute la page : le cadre, la
  // grille et la poignée y repassent par-dessus (popover), chaque fois qu'une fenêtre s'ouvre.
  let windowsOpen = -1;
  const raise = () => {
    const overlays = [gridView, box, hover, drop].filter(Boolean);
    if (!overlays.length || typeof overlays[0].showPopover !== "function") return;
    const count = document.querySelectorAll("dialog[open]").length;
    if (count === windowsOpen) return;
    windowsOpen = count;
    for (const el of overlays) {
      try {
        if (el.matches(":popover-open")) el.hidePopover();
        el.showPopover();
      } catch (_) { /* ancien navigateur : sous la fenêtre */ }
    }
  };
  const frame = () => {
    if (!box) return;
    raise();
    if (!selected || !selected.isConnected) {
      box.hidden = true;
      return;
    }
    const rect = selected.getBoundingClientRect();
    box.hidden = false;
    Object.assign(box.style, { left: `${rect.left - 2}px`, top: `${rect.top - 2}px`, width: `${rect.width + 4}px`, height: `${rect.height + 4}px` });
    box.querySelector("b").textContent = nameOf(selected);
    const place = peek(selected) && peek(selected).place;
    const parentGrid = selected.parentElement && peek(selected.parentElement) && peek(selected.parentElement).grid;
    box.querySelector("em").textContent = `${Math.round(rect.width)} × ${Math.round(rect.height)} px${place && parentGrid ? ` · ${place.span}/${parentGrid.columns} col.` : ""}`;
    drawGrid();
  };
  let gridView = null;
  const drawGrid = () => {
    if (!gridView) return;
    const owner = selected && (peek(selected) && peek(selected).grid ? selected : selected.parentElement && peek(selected.parentElement) && peek(selected.parentElement).grid ? selected.parentElement : null);
    const metrics = owner && host && host.showGrid !== false ? gridMetrics(owner) : null;
    if (!metrics) {
      gridView.hidden = true;
      return;
    }
    gridView.hidden = false;
    Object.assign(gridView.style, {
      left: `${metrics.left}px`, top: `${metrics.rect.top}px`, width: `${metrics.width}px`, height: `${metrics.rect.height}px`,
      gridTemplateColumns: `repeat(${metrics.n}, minmax(0, 1fr))`, columnGap: `${metrics.gap}px`,
    });
    // Les lignes de la grille : la bande de chaque ligne, sa hauteur, et l'écart entre deux lignes.
    const bands = new Map();
    for (const child of childrenOf(owner)) {
      const place = peek(child) && peek(child).place;
      if (!place) continue;
      const rect = child.getBoundingClientRect();
      const known = bands.get(place.row);
      bands.set(place.row, known ? { top: Math.min(known.top, rect.top), bottom: Math.max(known.bottom, rect.bottom) } : { top: rect.top, bottom: rect.bottom });
    }
    const rows = [...bands.entries()].sort((a, b) => a[0] - b[0]).map(([row, band]) => `<s style="top:${Math.round(band.top - metrics.rect.top)}px;height:${Math.round(band.bottom - band.top)}px"><b>L${row} · ${Math.round(band.bottom - band.top)} px</b></s>`).join("");
    const html = `${Array.from({ length: metrics.n }, (_, index) => `<span>${index === 0 || metrics.n <= 6 ? `<b>${Math.round(metrics.col)} px</b>` : ""}</span>`).join("")}${rows}<i>${metrics.n} colonnes · ${bands.size} lignes · ${Math.round(metrics.width)} px · écart ${Math.round(metrics.gap)} px</i>`;
    if (gridView.__html !== html) {
      gridView.innerHTML = html;
      gridView.__html = html;
    }
  };
  const select = (el) => {
    selected = el;
    frame();
    if (host && host.onSelect) host.onSelect(el ? info(el) : null);
  };
  let hover = null;
  let drop = null;
  let gesture = null;
  const onDown = (event) => {
    if (!host || host.mode !== "select") return;
    const handle = event.target.closest && event.target.closest(".claw-edit-box i");
    const mover = event.target.closest && event.target.closest(".claw-edit-box u");
    event.preventDefault();
    event.stopPropagation();
    if (mover && selected) {
      const metrics = selected.parentElement && gridMetrics(selected.parentElement);
      const rect = selected.getBoundingClientRect();
      gesture = { kind: "drag", el: selected, x: event.clientX, y: event.clientY,
        grab: metrics ? Math.floor((event.clientX - rect.left) / (metrics.col + metrics.gap)) : 0 };
      return;
    }
    if (handle && selected) {
      const rect = selected.getBoundingClientRect();
      gesture = { kind: "resize", x: event.clientX, y: event.clientY, w: rect.width, h: rect.height };
      return;
    }
    const el = pick(event.target);
    if (!el) return;
    // Un bouton déjà choisi, touché encore : il sert, comme dans l'appli (le + ouvre sa fenêtre,
    // la croix la ferme), sans quitter le mode Modifier.
    if (el === selected && el.matches("button, a[href], [role=button], [role=tab], summary, label, select")) {
      passing = true;
      try { el.click(); } finally { passing = false; }
      gesture = null;
      return;
    }
    if (el !== selected) select(el);
    // Au doigt, le bloc se déplace par sa poignée (le doigt posé sur l'appli la fait défiler).
    gesture = event.pointerType === "touch" ? null : { kind: "press", x: event.clientX, y: event.clientY, el, grab: 0 };
  };
  /** La case de la grille sous le doigt : colonne, ligne existante ou ligne neuve à ouvrir. */
  const gridTarget = (event) => {
    const el = gesture.el;
    const parent = el.parentElement;
    const metrics = parent && gridMetrics(parent);
    if (!metrics) return null;
    const mine = peek(el) && peek(el).place;
    const span = mine ? mine.span : metrics.n;
    const pointerCol = Math.floor((event.clientX - metrics.left) / (metrics.col + metrics.gap)) + 1;
    const col = Math.max(1, Math.min(metrics.n - span + 1, pointerCol - (gesture.grab || 0)));
    const rows = new Map();
    for (const child of childrenOf(parent)) {
      if (child === el) continue;
      const place = peek(child) && peek(child).place;
      if (!place) continue;
      const rect = child.getBoundingClientRect();
      const known = rows.get(place.row);
      rows.set(place.row, known ? { top: Math.min(known.top, rect.top), bottom: Math.max(known.bottom, rect.bottom) } : { top: rect.top, bottom: rect.bottom });
    }
    const sorted = [...rows.entries()].sort((a, b) => a[0] - b[0]);
    const x = metrics.left + (col - 1) * (metrics.col + metrics.gap);
    const w = span * metrics.col + (span - 1) * metrics.gap;
    const y = event.clientY;
    for (const [row, band] of sorted) {
      const edge = Math.min(18, (band.bottom - band.top) * 0.25);
      if (y < band.top + edge) return { col, row, insert: true, shape: { left: x, top: band.top - 4, width: w, height: 6 } };
      if (y <= band.bottom - edge) return { col, row, insert: false, shape: { left: x, top: band.top, width: w, height: band.bottom - band.top } };
    }
    const end = sorted.length ? sorted[sorted.length - 1] : null;
    return { col, row: end ? end[0] + 1 : 1, insert: true, shape: { left: x, top: (end ? end[1].bottom : metrics.rect.top) + 2, width: w, height: 6 } };
  };
  const dropTarget = (event) => {
    const el = gesture.el;
    const parent = el.parentElement;
    if (!parent) return null;
    const list = visualOrder(parent).filter((child) => child !== el);
    if (!list.length) return null;
    const rects = list.map((child) => child.getBoundingClientRect());
    const row = /row/.test(getComputedStyle(parent).flexDirection) && getComputedStyle(parent).display.includes("flex");
    let index = list.length;
    for (let i = 0; i < rects.length; i += 1) {
      const middle = row ? rects[i].left + rects[i].width / 2 : rects[i].top + rects[i].height / 2;
      if ((row ? event.clientX : event.clientY) < middle) { index = i; break; }
    }
    const ref = rects[Math.min(index, rects.length - 1)];
    const before = index < rects.length;
    return { index, line: row
      ? { left: (before ? ref.left : ref.right) - 2, top: ref.top, width: 3, height: ref.height }
      : { left: ref.left, top: (before ? ref.top : ref.bottom) - 2, width: ref.width, height: 3 } };
  };
  const onMove = (event) => {
    if (!host || host.mode !== "select") return;
    if (!gesture) {
      const el = pick(event.target);
      if (hover && el && el !== selected) {
        const rect = el.getBoundingClientRect();
        hover.hidden = false;
        Object.assign(hover.style, { left: `${rect.left}px`, top: `${rect.top}px`, width: `${rect.width}px`, height: `${rect.height}px` });
      } else if (hover) hover.hidden = true;
      return;
    }
    event.preventDefault();
    if (gesture.kind === "resize") {
      let width = Math.max(16, Math.round(gesture.w + event.clientX - gesture.x));
      const height = Math.max(8, Math.round((gesture.h + event.clientY - gesture.y) / 4) * 4);
      const metrics = selected.parentElement && gridMetrics(selected.parentElement);
      if (metrics) {
        const place = (peek(selected) && peek(selected).place) || { col: 1 };
        const span = Math.max(1, Math.min(metrics.n - place.col + 1, Math.round((width + metrics.gap) / (metrics.col + metrics.gap))));
        gesture.span = span;
        selected.style.setProperty("grid-column", `${place.col} / span ${span}`, "important");
        selected.style.setProperty("height", `${height}px`, "important");
        gesture.size = { height: `${height}px` };
        frame();
        return;
      }
      selected.style.setProperty("width", `${width}px`, "important");
      selected.style.setProperty("height", `${height}px`, "important");
      gesture.size = { width: `${width}px`, height: `${height}px` };
      frame();
      return;
    }
    if (gesture.kind === "press" && Math.hypot(event.clientX - gesture.x, event.clientY - gesture.y) > 8) gesture.kind = "drag";
    if (gesture.kind === "drag" && gesture.el.parentElement && gridMetrics(gesture.el.parentElement)) {
      const target = gridTarget(event);
      gesture.cell = target;
      if (target) {
        drop.hidden = false;
        drop.classList.toggle("cell", !target.insert);
        Object.assign(drop.style, { left: `${target.shape.left}px`, top: `${target.shape.top}px`, width: `${target.shape.width}px`, height: `${target.shape.height}px` });
      }
      return;
    }
    if (gesture.kind === "drag") {
      const target = dropTarget(event);
      gesture.to = target ? target.index : null;
      if (target) {
        drop.hidden = false;
        Object.assign(drop.style, { left: `${target.line.left}px`, top: `${target.line.top}px`, width: `${target.line.width}px`, height: `${target.line.height}px` });
      }
    }
  };
  const onUp = (event) => {
    if (!host || host.mode !== "select" || !gesture) return;
    event.preventDefault();
    event.stopPropagation();
    const done = gesture;
    gesture = null;
    if (drop) { drop.hidden = true; drop.classList.remove("cell"); }
    if (done.kind === "resize" && done.span && host.onPlace) host.onPlace(keyOf(selected), { span: done.span, height: done.size.height });
    else if (done.kind === "resize" && done.size && host.onResize) host.onResize(keyOf(selected), done.size);
    if (done.kind === "drag" && done.cell && host.onPlace) {
      host.onPlace(keyOf(done.el), { col: done.cell.col, row: done.cell.row, insert: done.cell.insert });
      return;
    }
    if (done.kind === "drag" && done.to != null && host.onReorder) host.onReorder(keyOf(done.el), done.to);
  };
  let passing = false;
  const swallow = (event) => {
    if (!host || host.mode !== "select" || passing) return;
    event.preventDefault();
    event.stopPropagation();
  };
  const edit = (next) => {
    host = next;
    if (!box) {
      ensureStyle("claw-design-editor", EDITOR_CSS);
      box = document.createElement("div");
      box.className = "claw-edit-box";
      box.setAttribute(OWN, "");
      box.innerHTML = `<b></b><em></em><i title="Taille"></i><u title="Déplacer">${icon("move", 18) || '<svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2v20M2 12h20M12 2l-3 3M12 2l3 3M12 22l-3-3M12 22l3-3M2 12l3-3M2 12l3 3M22 12l-3-3M22 12l-3 3"/></svg>'}</u>`;
      gridView = document.createElement("div");
      gridView.className = "claw-edit-grid";
      gridView.setAttribute(OWN, "");
      gridView.hidden = true;
      box.hidden = true;
      hover = document.createElement("div");
      hover.className = "claw-edit-hover";
      hover.setAttribute(OWN, "");
      hover.hidden = true;
      drop = document.createElement("div");
      drop.className = "claw-edit-drop";
      drop.setAttribute(OWN, "");
      drop.hidden = true;
      for (const el of [gridView, box, hover, drop]) if ("showPopover" in el) el.setAttribute("popover", "manual");
      document.documentElement.append(gridView, box, hover, drop);
      addEventListener("pointerdown", onDown, true);
      addEventListener("pointermove", onMove, { capture: true, passive: false });
      addEventListener("pointerup", onUp, true);
      for (const type of ["click", "dblclick", "submit", "touchstart", "mousedown", "contextmenu", "dragstart"]) addEventListener(type, swallow, { capture: true, passive: false });
      addEventListener("scroll", frame, true);
      addEventListener("resize", frame);
    }
    document.documentElement.classList.toggle("claw-editing", host.mode === "select");
    apply();
  };
  const setMode = (mode) => {
    if (!host) return;
    host.mode = mode;
    document.documentElement.classList.toggle("claw-editing", mode === "select");
    if (mode !== "select") {
      if (hover) hover.hidden = true;
      if (box) box.hidden = true;
      if (gridView) gridView.hidden = true;
    } else frame();
  };

  // Le fichier claw-design.json voisin de la page.
  const load = async () => {
    try {
      const response = await fetch(new URL("claw-design.json", document.baseURI), { cache: "no-store" });
      if (response.ok && !touched) design = normalize(await response.json());
    } catch (_) { /* pas de retouches */ }
    apply();
  };

  window.clawDesign = {
    version: VERSION, icons: Object.keys(ICONS), iconSvg: (name, size) => icon(name, size), fonts: Object.keys(FONTS),
    widgets: Object.fromEntries(Object.entries(WIDGETS).map(([type, kind]) => [type, { label: kind.label, props: kind.props }])),
    get design() { return JSON.parse(JSON.stringify(design)); },
    set(next) { touched = true; design = normalize(JSON.parse(JSON.stringify(next))); apply(); if (selected && !selected.isConnected) select(null); else frame(); },
    apply, keyOf, find, reorder, enableGrid, disableGrid, setGrid, placeAt, edit, setMode, refresh: () => frame(), info: (key) => { const el = find(key); return el ? info(el) : null; },
    select: (key) => select(key ? find(key) : null), normalize,
  };
  if (document.body) start(); else addEventListener("DOMContentLoaded", start);
  load();
})();
