// Kit d'appli de Claw pour React : l'allure et le fonctionnement communs d'une appli perso de
// téléphone (néobanque). Composez les écrans avec ces composants ; app-kit.css les habille, en
// clair et en sombre, aux couleurs de src/theme.css. Ne modifiez pas les fichiers de ce dossier.
//
// Cadre      AppShell (bar, tabs, onAdd, addLabel ; enfants : des Screen, écrits là ou dans vos
//            composants ; sur un ordinateur, les onglets passent dans une colonne à gauche et les
//            cartes côte à côte), Screen (id, title, onAdd : le bouton + quand cet écran est ouvert),
//            MonthBar (mois affiché), AppBar (title), IconButton (icon, label, onClick | href),
//            useScreen() → [écran ouvert, go("id")].
// Blocs      Hero (label, amount | value, sub, pills [{ label, value, note }], progress
//            { value 0..1, label, start, end }), Tiles > Tile (icon, label, value, note),
//            Card (title, meta, link { label, href | onClick }, level), List > Row (key, icon,
//            avatar (un prénom : ses initiales), tone "in" | "out" | "accent" | "warn", title, badge, sub, amount, end (un bouton,
//            sous le montant), endNote, action { icon, label, onClick }), Badge (tone), Meters > Meter (name, value 0..1,
//            valueText, note, tone), Empty (icon, text, steps, action), Avatar (name), Amount (value : un montant qui
//            défile quand il change).
// Données    useEntries (zustand, gardé sur le téléphone : entries, month, add, update,
//            remove → annuler, clear → annuler, setMonth), seedOnce([...]) au démarrage,
//            useMonthEntries(), useMonthTotals() → { income, spending, balance }, EntryList
//            (limit, kind, category, emptyText), persistentStore("nom", { … }) pour les
//            réglages, exportCsv(), pickCsv().
// Fenêtres   EntrySheet (categories [{ name, icon }], kinds, quick), openEntrySheet(entry?),
//            Sheet, Modal, ConfirmModal (open, onClose, title, text, confirmLabel, onConfirm),
//            ItemActions (name, onEdit, onDelete : Modifier, et Supprimer qui demande confirmation).
// Champs     Button (variant "primary" | "danger", small, block, icon), ButtonRow, Segmented,
//            Chips (value ; une liste pour plusieurs choix : pour qui), TextField (type
//            "number" garde la virgule), MoneyInput, Switch, Settings > Setting (name, note),
//            Select (label, value, options, placeholder), DateField (label, value « 2026-10-28 »),
//            SearchField (value, onChange), AddField (label, onAdd(texte) → une erreur ou rien :
//            un prénom, une habitude), Form (onSubmit() → une erreur ou rien, submitLabel).
// Suivis     WeekChecks (done [jours], onToggle(jour), label : les 7 derniers jours à cocher),
//            currentStreak(done) (jusqu'à hier tant qu'aujourd'hui n'est pas coché),
//            bestStreak(done), successRate(done) → 43.
// Widgets   Tabs (tabs [{ id, label, content }] : dans un écran), Accordion (items), Combobox
//            (label, value, options : on cherche en tapant), Menu (label, items [{ label, icon,
//            onSelect, danger }] : le « ⋯ »), Hint (text : infobulle), DataTable (columns, rows,
//            rowKey : triable, paginé), Skeleton, Spinner, Steps (steps, current), QuantityInput,
//            Rating, Slider, CodeInput (code reçu par SMS), FileDrop (onFiles), MonthCalendar
//            (value, onChange, marked : jours marqués).
// Encore     TextArea (max : caractères restants), PasswordField (strength : solidité),
//            Alert (tone "info" | "success" | "warn" | "error", title, action, onClose), Chat
//            (messages [{ id, mine, name, text, time }], onSend), Timeline (items [{ id, title,
//            time, text, icon, tone, done }]), Carousel (items, label), Countdown (to, label),
//            Stat (label, value, delta en %, goodWhenUp), DetailList (items [{ label, value }]),
//            CopyButton (text), ToggleButton (pressed, onChange, icon, label : favori),
//            LockScreen (check(code), length : autour d'AppShell).
// Téléphone  SwipeRow (comme Row, + onDelete à gauche, onDone à droite), Checklist (items
//            [{ id, text, done }], onChange), TagsField (value [textes], suggestions), ColorPicker,
//            IconPicker (value : un nom de ICONS ; iconByName(nom) le dessine), Timer (minutes,
//            onDone), Stopwatch, YearHeatmap (values : jours faits ou { jour: nombre }),
//            DonutChart (data [{ name, value }], center, format), Sparkline (values),
//            SortableList (items, onReorder, itemKey, itemLabel, render), PhotoField (value :
//            texte de shrinkPhoto), ShareButton (title, text, url), InstallBanner.
// Animations Appear (delay, onScroll : en défilant), Stagger (ses enfants l'un après l'autre),
//            Reveal (open : se déplie), Swap (id, direction ±1 : le mois suivant glisse), Pop et
//            Shake (trigger : rebond, secousse d'erreur), Pulse (active : halo), SuccessCheck,
//            ProgressRing (value 0..1, label), celebrate(big) (confettis). Toutes se calment
//            quand le téléphone demande moins de mouvement.
// Argent     RecurringList (kind "in" | "out" : salaire, loyer… posés une fois dans chaque mois,
//            jamais en double, jamais dans les mois d'avant), useRecurring, recurringTotal(kind),
//            BankImport (relevé .csv de la banque, sans doublon entre relevés qui se chevauchent),
//            importBankCsv(fichier), parseBankCsv(texte), categorize(libellé) (CARREFOUR →
//            Courses ; ce que l'utilisateur a choisi d'abord), recategorize(opération, catégorie)
//            (la retient), MoneyCalendar (les jours de rentrée et de dépense du mois, un jour
//            touché montre ses opérations). N'écrivez jamais ces choses vous-même.
// Animations+ (anim.tsx) TextReveal (text : les mots apparaissent un à un), ScrambleText (text :
//            les lettres se mélangent puis se posent), Aurora (un fond aux couleurs qui bougent,
//            ses enfants devant), GlowCard (une lumière suit le doigt), Magnetic (un bouton
//            attiré par le doigt), Wiggle (active, every : se secoue pour être remarqué),
//            ExpandCard (children, detail, label : la carte s'agrandit en plein écran),
//            AnimatedList (items, itemKey, render : des blocs faits à la main qui arrivent,
//            partent et se replacent ; pour des Row, List le fait déjà), LoadingBar (active : barre de chargement en haut), Twinkle (étoiles qui
//            scintillent autour), Breathe (inhale, hold, exhale : cercle de respiration),
//            TypingDots (« en train d'écrire »), LiquidFill (value 0..1 : un verre qui se
//            remplit), DrawPath (d : un trait qui se dessine), Particles (kind "snow" | "hearts"
//            | "leaves" | "confetti" : qui tombent sur l'écran), Bump (value : grossit, vert ou
//            rouge, quand le nombre change).
// Effets   (fx.tsx) Typewriter (text : s'écrit lettre à lettre), Marquee (items, label : un
//            bandeau qui défile), Tilt (carte qui s'incline sous le doigt), RippleButton (une
//            onde au toucher), ShineText (un reflet passe sur un titre), FlipNumber (value : les
//            chiffres tournent), CountUp (to : compte jusqu'au chiffre), Parallax, Float (flotte
//            doucement), Flash (value : un éclat quand elle change), SwipeDeck (items, render,
//            onSwipe(item, "yes" | "no") : des cartes à trier à droite ou à gauche), SlideView
//            (page, back : l'écran suivant glisse), ScrollProgress (barre de lecture en haut).
// Encore     (plus.tsx) Drawer (open, onClose, title, side : panneau latéral), Popover (trigger,
//            label : une bulle d'options), NumberPad (value, onChange : clavier de montant) et
//            padAmount(value), TimePicker (heure « 08:30 »), Kanban (columns, cards, onMove :
//            tâches en colonnes), Agenda (items { id, date, time, title } : groupés par jour),
//            Gauge (value 0..1, label : jauge en demi-cercle), GoalCard (title, saved, target,
//            until « 2027-06 » : objectif d'épargne, combien par mois), PullToRefresh
//            (onRefresh), LoadMore (items, render : « Voir plus »), Pagination (page, pages,
//            onPage), Breadcrumbs (items), Lightbox (photos { src, alt } : vignettes ouvertes en
//            grand), SignaturePad (onChange(image)), useOnline() et OfflineBanner (réseau perdu),
//            SafeArea (une partie qui plante n'emporte pas l'écran : « Réessayer »), IconBadge
//            (icon, count, label : cloche à pastille).
// Gestes   (touch.tsx) FlipCard (front, back, label : une carte qui se retourne), HoldButton
//            (onDone : maintenir pour un geste sans retour), SpeedDial (actions : le + qui s'ouvre
//            en plusieurs actions), LikeButton (liked, onChange, count : un cœur qui éclate),
//            Onboarding (slides : les écrans du premier lancement, une seule fois), Tour (steps
//            { target: sélecteur, title, text } : visite guidée), Stories (items : des pages
//            qui avancent seules), useLongPress(action) (appui long).
// Vues      (views.tsx) WeekStrip (value, onChange, marked : les 7 jours d'une semaine),
//            RangeSlider (value [de, à]), DateRange (value { from, to } : une période et ses
//            raccourcis) et inPeriod(date, période), StackedBar (parts : une barre de
//            répartition), CompareBars (rows { name, planned, actual } : prévu face au réel),
//            Poll (question, options, votes, voted, onVote : un sondage), Leaderboard (rows
//            { name, score }, me : un classement), AvatarGroup (names), Highlight (text,
//            query : les mots cherchés surlignés), Wizard (steps { title, content, check } :
//            un formulaire en plusieurs étapes).
// Serveur    useApi(url) → { data, loading, error, reload } et sendApi(url, "POST", corps) pour
//            l'API Express ou FastAPI des projets node-api et python-api ; useLive(url, event,
//            initial) → { value, connected, emit } pour le temps réel Socket.IO ; useWasm()
//            → { exports, error } pour le module Rust en WebAssembly du projet rust-wasm. Une
//            appli publiée sur GitHub Pages n'a pas de serveur : useApi et useLive y affichent
//            leur erreur claire, les données restent sur le téléphone (useEntries).
// Graphiques BarChart (labels, series [{ name, tone, values }]), MonthsChart (months) ; recharts
//            pour le reste.
// Messages   toast(message, { tone: "error", action: { label, run } }).
// Formats    money(1235.15) « 1 235,15 € », signed(), parseAmount("12,50"), today(), monthOf(),
//            shiftMonth(), monthName(), shortMonth(), dayLabel(), daysLeft(), shiftDay(jour, ±n),
//            lastDays(7), weekdayLabel() « lun. 26 », longDayLabel(). Un jour vient toujours de
//            today() ou shiftDay(), jamais de toISOString() (la veille entre minuit et 2 h).
// Montants   partout en euros, un nombre à virgule (12.5 pour 12,50 €) : Hero, Tile, Row et
//            Amount, money(), signed(), useEntries. Une appli qui compte en centimes les divise
//            par 100 avant de les donner au kit (5149 centimes tels quels : « 5 149,00 € »).
// Icônes     Icon (icon, size), iconFor(catégorie) ; toutes les icônes de lucide-react.
//
// Aussi installés, à importer directement : framer-motion (mouvements, motion.div), recharts
// (courbes, camemberts), date-fns et date-fns/locale/fr (dates), zod (vérifier une saisie),
// radix-ui (menus, onglets, curseurs, infobulles accessibles, à habiller avec Tailwind),
// canvas-confetti (célébrer un objectif atteint), clsx (classes conditionnelles).

export * from "./anim";
export * from "./blocks";
export * from "./chart";
export * from "./csv";
export * from "./entries";
export * from "./extras";
export * from "./format";
export * from "./forms";
export * from "./fx";
export * from "./icons";
export * from "./layout";
export * from "./money";
export * from "./more";
export * from "./motion";
export * from "./offline";
export * from "./plus";
export * from "./settings";
export * from "./sheet";
export * from "./store";
export * from "./toast";
export * from "./touch";
export * from "./tracker";
export * from "./views";
export * from "./widgets";
