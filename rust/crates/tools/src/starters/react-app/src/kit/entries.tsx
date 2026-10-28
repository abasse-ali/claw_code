// Les opérations du mois affiché : la liste qui se remplit seule, et ses totaux.

import { Pencil, Receipt } from "lucide-react";
import { useMemo } from "react";
import { dayLabel } from "./format";
import { iconFor } from "./icons";
import { Empty, List, Row } from "./blocks";
import { openEntrySheet } from "./sheet";
import { inMonth, newestFirst, totals, useEntries, type Entry } from "./store";

/** Les opérations du mois affiché (ou de `month`, ou de tous les mois avec "all"). */
export function useMonthEntries(month?: string | "all"): Entry[] {
  const entries = useEntries((state) => state.entries);
  const shown = useEntries((state) => state.month);
  const wanted = month ?? shown;
  return useMemo(() => (wanted === "all" ? entries : inMonth(entries, wanted)), [entries, wanted]);
}

/** Rentrées, dépenses et solde du mois affiché (ou de `month`). */
export function useMonthTotals(month?: string | "all") {
  const entries = useMonthEntries(month);
  return useMemo(() => totals(entries), [entries]);
}

type EntryListProps = {
  /** Les plus récentes seulement (4 sur l'accueil). */
  limit?: number;
  /** Les rentrées ("in") ou les dépenses ("out") seulement. */
  kind?: "in" | "out";
  category?: string;
  month?: string | "all";
  /** La phrase quand il n'y a rien (« Aucune opération ce mois-ci. »). */
  emptyText?: string;
  label?: string;
};

/** La liste des opérations, la plus récente d'abord ; une ligne touchée s'ouvre pour être modifiée. */
export function EntryList({ limit, kind, category, month, emptyText = "Aucune opération ce mois-ci.", label }: EntryListProps) {
  const entries = useMonthEntries(month);
  const shown = useMemo(() => {
    let list = entries;
    if (kind === "in") list = list.filter((entry) => entry.amount > 0);
    if (kind === "out") list = list.filter((entry) => entry.amount < 0);
    if (category) list = list.filter((entry) => entry.category === category);
    return newestFirst(list).slice(0, limit);
  }, [entries, kind, category, limit]);
  if (!shown.length) {
    return <Empty icon={Receipt} text={emptyText} action={{ label: "Ajouter une opération", onClick: () => openEntrySheet() }} />;
  }
  return (
    <List label={label}>
      {shown.map((entry) => (
        <Row
          key={entry.id}
          icon={iconFor(entry.category, entry.label)}
          tone={entry.amount > 0 ? "in" : "accent"}
          title={entry.label}
          sub={[entry.category !== entry.label ? entry.category : "", dayLabel(entry.date)].filter(Boolean).join(" · ")}
          amount={entry.amount}
          action={{ icon: Pencil, label: `Modifier « ${entry.label} »`, onClick: () => openEntrySheet(entry) }}
        />
      ))}
    </List>
  );
}
