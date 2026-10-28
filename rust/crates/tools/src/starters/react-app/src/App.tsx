// L'appli : ses données de départ, ses réglages et ses écrans, composés avec le kit.
// Exemple d'un budget du mois ; changez les textes, les écrans, les catégories et les
// chiffres pour l'appli demandée, et retirez ce qui ne sert pas.

import {
  Bus,
  ChartColumn,
  Download,
  Gamepad2,
  Gift,
  HeartPulse,
  House,
  List as ListIcon,
  Receipt,
  Settings as SettingsIcon,
  ShoppingBag,
  ShoppingCart,
  Tag,
  Trash2,
  Upload,
  Utensils,
  Wallet,
} from "lucide-react";
import { useMemo, useState } from "react";
import {
  AppShell,
  Button,
  ButtonRow,
  Card,
  ConfirmModal,
  EntryList,
  EntrySheet,
  Hero,
  IconButton,
  Meter,
  Meters,
  MoneyInput,
  MonthBar,
  MonthsChart,
  Screen,
  Setting,
  Settings,
  Tile,
  Tiles,
  daysLeft,
  exportCsv,
  money,
  openEntrySheet,
  persistentStore,
  pickCsv,
  seedOnce,
  toast,
  useEntries,
  useMonthEntries,
  useMonthTotals,
  type Category,
  type NewEntry,
} from "./kit";

// Les chiffres que l'utilisateur a donnés (libellé, montant négatif pour une dépense,
// catégorie) : dans l'appli dès le premier lancement, au mois en cours. Jamais inventés.
const SEED: NewEntry[] = [];
seedOnce(SEED);

// Les chiffres qu'il ajuste dans les réglages.
const useBudget = persistentStore("budget", { monthly: 0 });

const CATEGORIES: Category[] = [
  { name: "Courses", icon: ShoppingCart },
  { name: "Sorties", icon: Utensils },
  { name: "Transport", icon: Bus },
  { name: "Shopping", icon: ShoppingBag },
  { name: "Santé", icon: HeartPulse },
  { name: "Loisirs", icon: Gamepad2 },
  { name: "Cadeaux", icon: Gift },
  { name: "Autre", icon: Tag },
];

const TABS = [
  { id: "accueil", label: "Accueil", icon: House },
  { id: "operations", label: "Opérations", icon: ListIcon },
  { id: "bilan", label: "Bilan", icon: ChartColumn },
  { id: "reglages", label: "Réglages", icon: SettingsIcon },
];

export default function App() {
  const monthly = useBudget((state) => state.monthly);
  const month = useEntries((state) => state.month);
  const clear = useEntries((state) => state.clear);
  const entries = useMonthEntries();
  const { income, spending } = useMonthTotals();
  const [confirm, setConfirm] = useState(false);

  const left = monthly - spending;
  const days = daysLeft(month);
  const byCategory = useMemo(() => {
    const sums = new Map<string, number>();
    for (const entry of entries) if (entry.amount < 0) sums.set(entry.category, (sums.get(entry.category) ?? 0) - entry.amount);
    return [...sums].sort((a, b) => b[1] - a[1]);
  }, [entries]);

  return (
    <>
      <AppShell
        bar={<MonthBar><IconButton icon={SettingsIcon} label="Réglages" href="#reglages" /></MonthBar>}
        tabs={TABS}
        onAdd={() => openEntrySheet()}
        addLabel="Ajouter une opération"
      >
        <Screen id="accueil" label="Accueil">
          <Hero
            label="Reste à dépenser"
            amount={left}
            sub={<>Budget du mois : <b>{money(monthly)}</b></>}
            pills={[
              { label: "Par jour", value: money(Math.max(0, left) / days), note: `${days} jours restants` },
              { label: "Dépensé", value: money(spending), note: "ce mois-ci" },
            ]}
            progress={{
              value: monthly > 0 ? spending / monthly : 0,
              label: "Budget du mois déjà dépensé",
              start: `${monthly > 0 ? Math.round((spending / monthly) * 100) : 0} % du budget dépensé`,
              end: money(monthly),
            }}
          />
          <Tiles>
            <Tile icon={Wallet} label="Rentrées" value={money(income)} note="ce mois-ci" />
            <Tile icon={Receipt} label="Opérations" value={entries.length} note="ce mois-ci" />
          </Tiles>
          <Card title="Dernières opérations" link={{ label: "Tout voir", href: "#operations" }}>
            <EntryList limit={4} />
          </Card>
        </Screen>

        <Screen id="operations" title="Opérations">
          <Tiles>
            <Tile label="Dépenses" value={money(spending)} note="ce mois-ci" />
            <Tile label="Rentrées" value={money(income)} note="ce mois-ci" />
          </Tiles>
          <Card>
            <EntryList label="Opérations du mois" />
          </Card>
        </Screen>

        <Screen id="bilan" title="Bilan">
          <Card title="Rentrées et dépenses" meta="6 derniers mois" level={3}>
            <MonthsChart months={6} />
          </Card>
          <Card title="Dépenses par catégorie" level={3}>
            {byCategory.length ? (
              <Meters>
                {byCategory.map(([name, sum]) => (
                  <Meter key={name} name={name} value={spending ? sum / spending : 0} valueText={money(sum)} tone="out" />
                ))}
              </Meters>
            ) : (
              <p>Aucune dépense ce mois-ci.</p>
            )}
          </Card>
        </Screen>

        <Screen id="reglages" title="Réglages">
          <Card title="Mon budget" level={3}>
            <Settings>
              <Setting name="Budget du mois" note="ce que je peux dépenser">
                <MoneyInput label="Budget du mois" value={monthly} onChange={(value) => useBudget.setState({ monthly: value })} />
              </Setting>
            </Settings>
          </Card>
          <Card title="Mes données" level={3}>
            <p>Elles restent sur ce téléphone. Gardez-en une copie de temps en temps.</p>
            <div className="mt-3 grid gap-2.5">
              <ButtonRow>
                <Button small icon={Download} onClick={exportCsv}>Copie</Button>
                <Button small icon={Upload} onClick={pickCsv}>Reprendre</Button>
              </ButtonRow>
              <Button small block variant="danger" icon={Trash2} onClick={() => setConfirm(true)}>Tout effacer</Button>
            </div>
          </Card>
        </Screen>
      </AppShell>

      <EntrySheet categories={CATEGORIES} />
      <ConfirmModal
        open={confirm}
        onClose={() => setConfirm(false)}
        title="Tout effacer ?"
        text="Les opérations de tous les mois seront effacées de ce téléphone."
        confirmLabel="Effacer"
        onConfirm={() => {
          const undo = clear();
          toast("Toutes les opérations sont effacées", { action: { label: "Annuler", run: undo } });
        }}
      />
    </>
  );
}
