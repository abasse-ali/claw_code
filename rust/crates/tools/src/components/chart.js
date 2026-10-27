// Graphique en barres dessiné en SVG, à la largeur de son conteneur : il se
// redessine quand son écran s'ouvre (« screenchange »), quand sa taille change
// et quand les données changent (« entries:change »). Un graphique dessiné
// dans un écran caché ferait 0 px de large et resterait vide.
// data-chart="entries" : rentrées et dépenses des N derniers mois de
// window.entries ; un mois sans opération reste vide, jamais recopié.
// Ou des données à soi : window.drawChart(figure, { labels, series: [{ name,
// values, tone: "income" | "spending" }] }).
(() => {
  if (window.drawChart) return;
  const svgNs = "http://www.w3.org/2000/svg";
  const lang = document.documentElement.lang || "fr";
  const own = new WeakMap();
  const drawnAt = new WeakMap();
  const make = (name, attributes, parent) => {
    const node = document.createElementNS(svgNs, name);
    for (const [key, value] of Object.entries(attributes)) node.setAttribute(key, value);
    if (parent) parent.append(node);
    return node;
  };
  const fromEntries = (figure) => {
    const count = Math.max(1, Math.min(24, Number(figure.dataset.chartMonths) || 6));
    const now = new Date();
    const months = [];
    for (let back = count - 1; back >= 0; back--) months.push(new Date(now.getFullYear(), now.getMonth() - back, 1));
    const monthName = new Intl.DateTimeFormat(lang, { month: "short" });
    const key = (date) => `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}`;
    const sums = months.map((date) => (window.entries ? window.entries.totals(window.entries.month(key(date))) : { income: 0, spending: 0 }));
    return {
      labels: months.map((date) => monthName.format(date).replace(".", "")),
      series: [
        { name: "Rentrées", tone: "income", values: sums.map((sum) => sum.income) },
        { name: "Dépenses", tone: "spending", values: sums.map((sum) => sum.spending) },
      ],
    };
  };
  const nice = (value) => {
    if (value <= 0) return 1;
    const step = 10 ** Math.floor(Math.log10(value));
    return [1, 2, 2.5, 5, 10].map((factor) => factor * step).find((top) => top >= value);
  };
  const draw = (figure) => {
    const plot = figure.querySelector("[data-chart-plot]") || figure;
    const width = Math.round(plot.clientWidth);
    // Écran caché : rien à mesurer, il sera dessiné à son ouverture.
    if (width < 40) return;
    const data = own.get(figure) || (figure.dataset.chart === "entries" ? fromEntries(figure) : null);
    if (!data) return;
    const empty = figure.querySelector("[data-chart-empty]");
    const anything = data.series.some((serie) => serie.values.some((value) => value > 0));
    if (empty) empty.hidden = anything;
    const height = Number(figure.dataset.chartHeight) || 220;
    const money = new Intl.NumberFormat(lang, { style: "currency", currency: figure.dataset.currency || "EUR", maximumFractionDigits: 0 });
    const top = anything ? nice(Math.max(...data.series.flatMap((serie) => serie.values))) : 100;
    const left = Math.min(72, 12 + money.format(top).length * 7);
    const bottom = 28;
    const inner = height - bottom - 8;
    const svg = make("svg", { viewBox: `0 0 ${width} ${height}`, width, height, role: "img" });
    const summary = data.labels.map((label, index) => `${label} : ${data.series.map((serie) => `${serie.name.toLowerCase()} ${money.format(serie.values[index] || 0)}`).join(", ")}`).join(" ; ");
    svg.setAttribute("aria-label", summary);
    for (let step = 0; step <= 4; step++) {
      const y = 8 + inner - (inner * step) / 4;
      make("line", { class: "chart__grid", x1: left, x2: width, y1: y, y2: y }, svg);
      const text = make("text", { class: "chart__axis", x: left - 8, y: y + 4, "text-anchor": "end" }, svg);
      text.textContent = money.format((top * step) / 4);
    }
    const slot = (width - left) / data.labels.length;
    const bar = Math.max(4, Math.min(28, (slot * 0.7) / data.series.length));
    data.labels.forEach((label, index) => {
      const middle = left + slot * index + slot / 2;
      data.series.forEach((serie, which) => {
        const value = Math.max(0, serie.values[index] || 0);
        if (!value) return;
        const tall = Math.max(2, (inner * value) / top);
        const x = middle - (bar * data.series.length) / 2 + bar * which;
        const rect = make("rect", { class: `chart__bar chart__bar--${serie.tone || "income"}`, x: x + 1, y: 8 + inner - tall, width: bar - 2, height: tall, rx: 3 }, svg);
        make("title", {}, rect).textContent = `${serie.name}, ${label} : ${money.format(value)}`;
      });
      const text = make("text", { class: "chart__axis", x: middle, y: height - 8, "text-anchor": "middle" }, svg);
      text.textContent = label;
    });
    plot.replaceChildren(svg);
    drawnAt.set(figure, width);
  };
  const all = () => document.querySelectorAll("[data-chart]");
  window.drawChart = (figure, data) => { own.set(figure, data); draw(figure); };
  // Redessiné hors du rappel de l'observateur, et seulement quand la largeur
  // change : sinon le navigateur signale une boucle (« ResizeObserver loop »).
  const resized = typeof ResizeObserver === "function" ? new ResizeObserver((items) => {
    const figures = items
      .map((item) => [item.target.closest("[data-chart]"), Math.round(item.contentRect.width)])
      .filter(([figure, width]) => figure && width !== drawnAt.get(figure));
    if (figures.length) requestAnimationFrame(() => figures.forEach(([figure]) => draw(figure)));
  }) : null;
  const start = () => {
    for (const figure of all()) {
      if (figure.dataset.ready) continue;
      figure.dataset.ready = "1";
      if (resized) resized.observe(figure.querySelector("[data-chart-plot]") || figure);
      draw(figure);
    }
  };
  document.addEventListener("screenchange", () => requestAnimationFrame(() => all().forEach(draw)));
  document.addEventListener("entries:change", () => all().forEach(draw));
  window.addEventListener("resize", () => all().forEach(draw));
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start);
  else start();
})();
