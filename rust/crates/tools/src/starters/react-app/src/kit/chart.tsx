// Graphique en barres en SVG, à la largeur de sa carte, aux couleurs de l'appli (rentrées en
// vert, dépenses en rouge). Les noms des mois s'espacent quand la place manque. Pour d'autres
// graphiques (courbes, camemberts), recharts est installé : import { LineChart } from "recharts".

import { useEffect, useMemo, useRef, useState } from "react";
import { shiftMonth, shortMonth } from "./format";
import { inMonth, totals, useEntries } from "./store";

export type Series = { name: string; tone: "in" | "out"; values: number[] };

const lang = document.documentElement.lang || "fr";
const currency = document.documentElement.dataset.currency || "EUR";
const axisFormat = new Intl.NumberFormat(lang, { style: "currency", currency, notation: "compact", maximumFractionDigits: 1 });
const fullFormat = new Intl.NumberFormat(lang, { style: "currency", currency });

const niceTop = (value: number) => {
  if (value <= 0) return 100;
  const step = 10 ** Math.floor(Math.log10(value));
  return [1, 2, 2.5, 5, 10].map((factor) => factor * step).find((top) => top >= value) ?? value;
};

/** La largeur de l'élément, suivie quand l'écran change de taille. */
function useWidth<T extends HTMLElement>() {
  const ref = useRef<T>(null);
  const [width, setWidth] = useState(0);
  useEffect(() => {
    const node = ref.current;
    if (!node) return;
    setWidth(Math.round(node.clientWidth));
    const observer = new ResizeObserver(([entry]) => setWidth(Math.round(entry.contentRect.width)));
    observer.observe(node);
    return () => observer.disconnect();
  }, []);
  return [ref, width] as const;
}

type BarChartProps = {
  labels: string[];
  series: Series[];
  height?: number;
  /** La phrase quand toutes les valeurs sont à zéro. */
  emptyText?: string;
};

/** Barres groupées par libellé (un mois), une couleur par série, avec sa légende. */
export function BarChart({ labels, series, height = 200, emptyText = "Le graphique se remplit au fil des mois." }: BarChartProps) {
  const [plot, width] = useWidth<HTMLDivElement>();
  const anything = series.some((serie) => serie.values.some((value) => value > 0));
  const top = niceTop(anything ? Math.max(...series.flatMap((serie) => serie.values)) : 0);
  const left = Math.min(64, 10 + axisFormat.format(top).length * 6.5);
  const bottom = 24;
  const inner = height - bottom - 8;
  const slot = labels.length ? (width - left) / labels.length : 0;
  const bar = Math.max(3, Math.min(14, (slot * 0.62) / Math.max(1, series.length)));
  const longest = Math.max(1, ...labels.map((label) => label.length));
  const every = Math.max(1, Math.ceil((longest * 6.6 + 8) / Math.max(1, slot)));
  const summary = labels
    .map((label, index) => `${label} : ${series.map((serie) => `${serie.name.toLowerCase()} ${fullFormat.format(serie.values[index] || 0)}`).join(", ")}`)
    .join(" ; ");
  return (
    <div className="chart">
      <div className="chart__plot" ref={plot} hidden={!anything}>
        {width > 40 && anything && (
          <svg viewBox={`0 0 ${width} ${height}`} width={width} height={height} role="img" aria-label={summary}>
            {[0, 1, 2, 3, 4].map((step) => {
              const y = 8 + inner - (inner * step) / 4;
              return (
                <g key={step}>
                  <line className="chart__grid" x1={left} x2={width} y1={y} y2={y} />
                  {step % 2 === 0 && (
                    <text className="chart__axis" x={left - 8} y={y + 4} textAnchor="end">{axisFormat.format((top * step) / 4)}</text>
                  )}
                </g>
              );
            })}
            {labels.map((label, index) => {
              const middle = left + slot * index + slot / 2;
              return (
                <g key={label + index}>
                  {series.map((serie, which) => {
                    const value = Math.max(0, serie.values[index] || 0);
                    if (!value) return null;
                    const tall = Math.max(3, (inner * value) / top);
                    const x = middle - (bar * series.length) / 2 + bar * which;
                    return (
                      <rect key={serie.name} className={`chart__bar chart__bar--${serie.tone}`} x={x + 1} y={8 + inner - tall} width={Math.max(2, bar - 2)} height={tall} rx={Math.min(4, (bar - 2) / 2)}>
                        <title>{`${serie.name}, ${label} : ${fullFormat.format(value)}`}</title>
                      </rect>
                    );
                  })}
                  {(labels.length - 1 - index) % every === 0 && (
                    <text className="chart__axis" x={middle} y={height - 6} textAnchor="middle">{label}</text>
                  )}
                </g>
              );
            })}
          </svg>
        )}
      </div>
      {anything ? (
        <p className="chart__legend">
          {series.map((serie) => <span key={serie.name} className={`chart__key chart__key--${serie.tone}`}>{serie.name}</span>)}
        </p>
      ) : (
        <p className="chart__empty">{emptyText}</p>
      )}
    </div>
  );
}

/** Rentrées et dépenses des derniers mois, jusqu'au mois affiché. */
export function MonthsChart({ months = 6 }: { months?: number }) {
  const entries = useEntries((state) => state.entries);
  const month = useEntries((state) => state.month);
  const data = useMemo(() => {
    const list = Array.from({ length: months }, (_, index) => shiftMonth(month, index - months + 1));
    const sums = list.map((item) => totals(inMonth(entries, item)));
    return {
      labels: list.map((item) => shortMonth(item).replace(".", "")),
      series: [
        { name: "Rentrées", tone: "in" as const, values: sums.map((sum) => sum.income) },
        { name: "Dépenses", tone: "out" as const, values: sums.map((sum) => sum.spending) },
      ],
    };
  }, [entries, month, months]);
  return <BarChart labels={data.labels} series={data.series} />;
}
