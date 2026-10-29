// Kit d'appli de Claw pour React : l'allure et le fonctionnement communs d'une appli perso de
// téléphone (néobanque). Composez les écrans avec ces composants ; app-kit.css les habille, en
// clair et en sombre, aux couleurs de src/theme.css. Ne modifiez pas les fichiers de ce dossier.
//
// Cadre      AppShell (bar, tabs, onAdd, addLabel ; enfants : des Screen ; sur un ordinateur, les
//            onglets passent dans une colonne à gauche et les cartes côte à côte), Screen (id, title),
//            MonthBar (mois affiché), AppBar (title), IconButton (icon, label, onClick | href),
//            useScreen() → [écran ouvert, go("id")].
// Blocs      Hero (label, amount | value, sub, pills [{ label, value, note }], progress
//            { value 0..1, label, start, end }), Tiles > Tile (icon, label, value, note),
//            Card (title, meta, link { label, href | onClick }, level), List > Row (key, icon,
//            tone "in" | "out" | "accent" | "warn", title, badge, sub, amount | end, endNote,
//            action { icon, label, onClick }), Badge (tone), Meters > Meter (name, value 0..1,
//            valueText, note, tone), Empty (icon, text, action), Amount (value : un montant qui
//            défile quand il change).
// Données    useEntries (zustand, gardé sur le téléphone : entries, month, add, update,
//            remove → annuler, clear → annuler, setMonth), seedOnce([...]) au démarrage,
//            useMonthEntries(), useMonthTotals() → { income, spending, balance }, EntryList
//            (limit, kind, category, emptyText), persistentStore("nom", { … }) pour les
//            réglages, exportCsv(), pickCsv().
// Fenêtres   EntrySheet (categories [{ name, icon }], kinds, quick), openEntrySheet(entry?),
//            Sheet, Modal, ConfirmModal (open, onClose, title, text, confirmLabel, onConfirm).
// Champs     Button (variant "primary" | "danger", small, block, icon), ButtonRow, Segmented,
//            Chips, TextField, MoneyInput, Switch, Settings > Setting (name, note).
// Graphiques BarChart (labels, series [{ name, tone, values }]), MonthsChart (months) ; recharts
//            pour le reste.
// Messages   toast(message, { tone: "error", action: { label, run } }).
// Formats    money(1235.15) « 1 235,15 € », signed(), parseAmount("12,50"), today(), monthOf(),
//            shiftMonth(), monthName(), shortMonth(), dayLabel(), daysLeft().
// Montants   partout en euros, un nombre à virgule (12.5 pour 12,50 €) : Hero, Tile, Row et
//            Amount, money(), signed(), useEntries. Une appli qui compte en centimes les divise
//            par 100 avant de les donner au kit (5149 centimes tels quels : « 5 149,00 € »).
// Icônes     Icon (icon, size), iconFor(catégorie) ; toutes les icônes de lucide-react.
//
// Aussi installés, à importer directement : framer-motion (mouvements, motion.div), recharts
// (courbes, camemberts), date-fns et date-fns/locale/fr (dates), zod (vérifier une saisie),
// radix-ui (menus, onglets, curseurs, infobulles accessibles, à habiller avec Tailwind),
// canvas-confetti (célébrer un objectif atteint), clsx (classes conditionnelles).

export * from "./blocks";
export * from "./chart";
export * from "./csv";
export * from "./entries";
export * from "./format";
export * from "./forms";
export * from "./icons";
export * from "./layout";
export * from "./offline";
export * from "./settings";
export * from "./sheet";
export * from "./store";
export * from "./toast";
