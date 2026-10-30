//! Outil `Starter` : des projets de départ prêts, avec leurs bibliothèques
//! installées, pour que le modèle s'en serve au lieu de tout coder à la main.
//! Décision de l'utilisateur (28 octobre 2026) : « je veux qu'il puisse
//! directement les utiliser au lieu de coder à la main », React + Vite par
//! défaut pour les applis.
//!
//! - `react-app` : React, TypeScript, Vite, Tailwind CSS, Framer Motion,
//!   lucide-react, zustand, recharts, radix-ui, date-fns, zod, et le kit
//!   d'appli de Claw en composants React (`src/kit/`, l'allure de
//!   `app-kit.css`). Construite dans `docs/`, la page que GitHub Pages publie.
//! - `node-api` : un serveur Express et `Socket.IO` en `TypeScript`, avec ses tests.
//! - `python-api` : un serveur `FastAPI` avec son `WebSocket` et ses tests (pytest).
//!
//! Les fichiers du kit (`src/kit/`) sont ceux de Claw : récrits à chaque appel.
//! Les autres ne sont écrits que s'ils manquent : un second appel ne défait
//! jamais le travail du modèle.

use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Deserialize)]
pub(crate) struct StarterInput {
    #[serde(default)]
    kind: Option<String>,
    /// Dossier du projet, relatif au dossier de travail (sa racine par défaut).
    #[serde(default)]
    dir: Option<String>,
}

macro_rules! files {
    ($kind:literal: $($path:literal),+ $(,)?) => {
        &[$(($path, include_str!(concat!("starters/", $kind, "/", $path)))),+]
    };
}

const REACT_APP: &[(&str, &str)] = files!("react-app":
    ".gitignore",
    "index.html",
    "package.json",
    "tsconfig.json",
    "vite.config.ts",
    "public/icon.svg",
    "public/manifest.webmanifest",
    "public/sw.js",
    "src/App.tsx",
    "src/main.tsx",
    "src/index.css",
    "src/theme.css",
    "src/kit/blocks.tsx",
    "src/kit/chart.tsx",
    "src/kit/csv.ts",
    "src/kit/entries.tsx",
    "src/kit/format.ts",
    "src/kit/forms.tsx",
    "src/kit/icons.tsx",
    "src/kit/index.ts",
    "src/kit/layout.tsx",
    "src/kit/offline.ts",
    "src/kit/settings.ts",
    "src/kit/sheet.tsx",
    "src/kit/store.ts",
    "src/kit/toast.tsx",
    "src/kit/tracker.tsx",
    "src/kit/widgets.tsx",
);

/// L'allure du kit, la même que celle des applis sans React.
const APP_KIT_CSS: (&str, &str) = (
    "src/kit/app-kit.css",
    include_str!("components/app-kit.css"),
);

const NODE_API: &[(&str, &str)] = files!("node-api":
    ".gitignore",
    "package.json",
    "tsconfig.json",
    "src/app.ts",
    "src/app.test.ts",
    "src/server.ts",
    "src/store.ts",
);

const PYTHON_API: &[(&str, &str)] = files!("python-api":
    ".gitignore",
    "pytest.ini",
    "requirements.txt",
    "app/__init__.py",
    "app/main.py",
    "app/store.py",
    "tests/__init__.py",
    "tests/test_main.py",
);

const REACT_GUIDE: &str = "React 19 + TypeScript + Vite 8 + Tailwind CSS 4 + Framer Motion, with lucide-react, zustand, recharts, radix-ui, date-fns, zod and canvas-confetti installed: import them, never write by hand what they do. The app is written in src/App.tsx, which starts as a working example (a monthly budget): rewrite its texts, screens, categories, seed data and settings for this app, and remove what it does not need. Compose every screen with Claw's app kit, imported from \"./kit\" (its full list is at the top of src/kit/index.ts): AppShell (bar, tabs, onAdd; its children are one Screen per tab, written in place or inside your own components) and Screen (id, title, onAdd: what the + button does while that screen is shown, such as opening the sheet that screen keeps in its own state), MonthBar or AppBar, Hero (the main figure on the app's gradient: label, amount, sub, pills, progress), Tiles > Tile, Card (title, meta, link) holding a List of Row (icon, tone, title, badge, sub, amount, end: buttons under the amount, action) or Meters > Meter, Badge, Empty, BarChart and MonthsChart, Settings > Setting with MoneyInput or Switch, Button, ButtonRow, Segmented, Chips (one choice, or several when its value is a list: who an expense is for), TextField (type \"number\" keeps the comma of 12,50), Select, DateField, SearchField, AddField (a field and its Ajouter button on one line: onAdd returns an error such as a duplicate name, or nothing), Form (onSubmit returns an error or nothing; shows it above the button), ItemActions (Modifier and Supprimer, with its own confirmation), Avatar and Row avatar (a person by initials), WeekChecks with currentStreak, bestStreak and successRate (the last 7 days to tick), shiftDay and lastDays (days in the phone's time: never toISOString for a day), Empty steps, and widgets.tsx: Tabs (inside a screen), Accordion, Combobox (a choice list searched by typing), Menu (the ⋯ menu), Hint (tooltip), DataTable (sortable, paged), Skeleton, Spinner, Steps, QuantityInput, Rating, Slider, CodeInput (one-time code), FileDrop, MonthCalendar (marked days), EntrySheet (the sheet to add or edit an entry, opened by openEntrySheet()), Sheet, Modal, ConfirmModal, toast(message, { action: { label: \"Annuler\", run } }). Data: useEntries (entries kept on the phone, add, update, remove and clear return an undo), seedOnce(SEED) puts the figures the user gave in the app at its first launch, useMonthEntries and useMonthTotals for the month shown, EntryList fills itself, persistentStore(\"name\", { ... }) keeps the user's settings; a store selector returns stored values (useTrip((s) => s.expenses)), never a value built at each read (s.getBalances(), .filter(), { … }), which makes zustand render again without end: derive figures with useMemo; money(), signed(), parseAmount() and dayLabel() write amounts and dates the French way. On a computer the kit gives the app its own layout (the tabs in a column on the left with the add button on top, two or more cards side by side): keep the screens made of the kit's blocks and let it lay them out, never a phone column stretched by hand. Every amount the kit takes or shows (Hero, Tile, Row and Amount, money(), signed(), useEntries) is in euros, 12.5 for 12,50 €: an app that counts in integer cents divides them by 100 before handing them to the kit (passed as they are, 5149 cents show \"5 149,00 €\"). The app's colors go in src/theme.css (tokens only: --accent and the gradient --hero-1, --hero-2, --hero-3, light and dark); Tailwind classes lay out what the kit lacks (grid, gap-3, mt-4) with the kit's colors (bg-surface, text-muted, text-accent, border-line); motion.div from framer-motion animates the rest. Never edit src/kit/ (Claw rewrites it), never write CSS for cards, lists, buttons or fields, never use emoji. Icons: any Lucide icon, import { Wallet } from \"lucide-react\", passed as icon={Wallet}. public/ holds the icon (icon.svg), the manifest and the offline service worker.";

const REACT_CHECK: &str = "Claw builds the app (npm run build, into docs/, the page GitHub Pages publishes) before every Screenshot, Browser or ViewPage of index.html, and shows the build errors: always open index.html, never a file of docs/, and never edit docs/. npm run typecheck checks the types, then builds; Claw runs it after your changes.";

const NODE_GUIDE: &str = "A Node.js server in TypeScript: Express 5 for the API (src/app.ts: GET, POST, PATCH and DELETE on /api/items, kept in data/items.json by src/store.ts), Socket.IO for live updates (every change is sent to the connected clients as the \"items\" event), cors, tsx to run TypeScript directly. Change the routes and the store for this project; keep createApp() returning { app, server, io, store } so that the tests can start it on a free port. npm start runs it (port 3000, or PORT), npm run dev reloads on every change, npm test runs src/app.test.ts (node:test, fetch and socket.io-client), npm run typecheck checks the types. A page in public/ is served with the API. GitHub Pages only publishes static pages: this server needs a host that runs Node (Render, Railway, Fly.io, a VPS).";

const PYTHON_GUIDE: &str = "A Python server: FastAPI for the API (app/main.py: GET, POST, PATCH and DELETE on /api/items, kept in data/items.json by app/store.py, input checked by pydantic models), a WebSocket on /ws for live updates (every change is sent to the connected clients), CORS open. Change the routes and the store for this project; keep create_app(store) so that the tests use an in-memory store. Its libraries are installed in .venv: .venv/bin/python -m pytest runs tests/test_main.py (TestClient), .venv/bin/uvicorn app.main:app --reload runs it on port 8000 (on Windows, .venv\\Scripts\\ instead of .venv/bin/). GitHub Pages only publishes static pages: this server needs a host that runs Python (Render, Railway, Fly.io, a VPS).";

struct Starter {
    kind: &'static str,
    purpose: &'static str,
    files: &'static [(&'static str, &'static str)],
    /// Fichiers de Claw : récrits à chaque appel.
    owned: &'static str,
    guide: &'static str,
}

const STARTERS: &[Starter] = &[
    Starter {
        kind: "react-app",
        purpose: "an app or interactive page in React + TypeScript + Vite + Tailwind CSS + Framer Motion, with Claw's app kit as React components; built into docs/ for GitHub Pages",
        files: REACT_APP,
        owned: "src/kit/",
        guide: REACT_GUIDE,
    },
    Starter {
        kind: "node-api",
        purpose: "a Node.js server: Express API, Socket.IO live updates, TypeScript, tests",
        files: NODE_API,
        owned: "",
        guide: NODE_GUIDE,
    },
    Starter {
        kind: "python-api",
        purpose: "a Python server: FastAPI API, WebSocket live updates, tests with pytest",
        files: PYTHON_API,
        owned: "",
        guide: PYTHON_GUIDE,
    },
];

fn find(kind: &str) -> Option<&'static Starter> {
    let key = kind.trim().to_ascii_lowercase().replace([' ', '_'], "-");
    let key = match key.as_str() {
        "" | "react" | "vite" | "app" | "appli" | "react-vite" | "web-app" | "webapp" | "pwa" => {
            "react-app"
        }
        "node" | "express" | "socket.io" | "socketio" | "api" | "server" | "node-server"
        | "express-api" => "node-api",
        "python" | "fastapi" | "py" | "python-server" | "fastapi-api" => "python-api",
        other => other,
    };
    STARTERS.iter().find(|starter| starter.kind == key)
}

/// Le dossier du projet, dans le dossier de travail `root`.
fn project_dir(root: &Path, dir: Option<&str>) -> Result<PathBuf, String> {
    let dir = dir
        .map(str::trim)
        .filter(|dir| !dir.is_empty())
        .unwrap_or(".");
    let relative = Path::new(dir.trim_start_matches("./"));
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(format!(
            "dir `{dir}` must be a folder inside the project, written relative to it (for example \".\" or \"server\")"
        ));
    }
    Ok(root.join(relative))
}

/// La feuille du modèle de conception à la racine du projet : `base.css`
/// (marquée par Claw), ou `claw-base.css` quand l'utilisateur avait déjà son
/// propre `base.css`.
fn designer_css(folder: &Path) -> Option<&'static str> {
    let ours = |name: &str| {
        std::fs::read_to_string(folder.join(name))
            .is_ok_and(|text| text.starts_with(runtime::web_design::BASE_CSS_MARK))
    };
    ["claw-base.css", "base.css"]
        .into_iter()
        .find(|name| ours(name))
}

/// `src/index.css` : la feuille du modèle de conception passe avant les
/// couleurs de l'appli quand elle existe.
fn react_index_css(folder: &Path, text: &str) -> String {
    match designer_css(folder) {
        Some(name) => text.replace(
            "@import \"./theme.css\";",
            &format!("@import \"../{name}\";\n@import \"./theme.css\";"),
        ),
        None => text.to_string(),
    }
}

/// Écrit les fichiers du projet de départ ; rend ceux écrits et ceux gardés.
fn write_files(starter: &Starter, folder: &Path) -> Result<(Vec<String>, Vec<String>), String> {
    let mut written = Vec::new();
    let mut kept = Vec::new();
    let extra: &[(&str, &str)] = if starter.kind == "react-app" {
        std::slice::from_ref(&APP_KIT_CSS)
    } else {
        &[]
    };
    for (path, text) in starter.files.iter().chain(extra) {
        let full = folder.join(path);
        let owned = !starter.owned.is_empty() && path.starts_with(starter.owned);
        if full.exists() && !owned {
            if *path == ".gitignore" {
                // Un .gitignore déjà là : les lignes du projet de départ qui y manquent.
                let current = std::fs::read_to_string(&full).unwrap_or_default();
                let missing: Vec<&str> = text
                    .lines()
                    .filter(|line| {
                        !line.trim().is_empty()
                            && !current.lines().any(|known| known.trim() == line.trim())
                    })
                    .collect();
                if !missing.is_empty() {
                    let joined = if current.ends_with('\n') || current.is_empty() {
                        ""
                    } else {
                        "\n"
                    };
                    std::fs::write(&full, format!("{current}{joined}{}\n", missing.join("\n")))
                        .map_err(|error| error.to_string())?;
                    written.push((*path).to_string());
                    continue;
                }
            }
            kept.push((*path).to_string());
            continue;
        }
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let text = if *path == "src/index.css" {
            react_index_css(folder, text)
        } else {
            (*text).to_string()
        };
        std::fs::write(&full, text).map_err(|error| error.to_string())?;
        written.push((*path).to_string());
    }
    Ok((written, kept))
}

/// `python -m venv .venv` puis l'installation des bibliothèques dedans.
fn python_install(folder: &Path) -> Result<(), String> {
    let quiet = |mut command: Command| {
        command
            .current_dir(folder)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
    };
    if !folder.join(".venv").join("pyvenv.cfg").is_file() {
        let mut made = None;
        for program in ["python3", "python"] {
            let mut command = Command::new(program);
            command.args(["-m", "venv", ".venv"]);
            match quiet(command) {
                Ok(output) if output.status.success() => {
                    made = Some(Ok(()));
                    break;
                }
                Ok(output) => {
                    made = Some(Err(String::from_utf8_lossy(&output.stderr)
                        .trim()
                        .to_string()));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => made = Some(Err(error.to_string())),
            }
        }
        match made {
            Some(Ok(())) => {}
            Some(Err(error)) => return Err(format!("python -m venv .venv failed: {error}")),
            None => return Err("Python is not installed on this computer".to_string()),
        }
    }
    let python = if cfg!(windows) {
        folder.join(".venv").join("Scripts").join("python.exe")
    } else {
        folder.join(".venv").join("bin").join("python")
    };
    let mut command = Command::new(python);
    command.args([
        "-m",
        "pip",
        "install",
        "--quiet",
        "--disable-pip-version-check",
        "-r",
        "requirements.txt",
    ]);
    match quiet(command) {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => Err(format!(
            "pip install -r requirements.txt failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(error) => Err(error.to_string()),
    }
}

/// Le chemin de `path` depuis le dossier de travail `root`.
fn from_root(root: &Path, folder: &Path, path: &str) -> String {
    let full = folder.join(path);
    full.strip_prefix(root)
        .unwrap_or(&full)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Ce que l'outil `Starter` rend au modèle.
pub(crate) fn starter_tool(input: &StarterInput) -> Result<Value, String> {
    let kind = input.kind.as_deref().unwrap_or("");
    let Some(starter) = find(kind) else {
        return Err(format!(
            "unknown starter `{kind}`: use one of {}",
            STARTERS
                .iter()
                .map(|starter| format!("{} ({})", starter.kind, starter.purpose))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    };
    let root = std::env::current_dir().map_err(|error| error.to_string())?;
    let folder = project_dir(&root, input.dir.as_deref())?;
    // Une page d'avant, sans React, à la place de la page de l'appli : elle
    // resterait la page publiée, et l'appli React ne s'ouvrirait jamais.
    if starter.kind == "react-app" {
        if let Ok(html) = std::fs::read_to_string(folder.join("index.html")) {
            if !html.contains("/src/main.tsx") {
                return Err("this folder already has an index.html that is not a React app (a page made without the starter): keep building that page as it is, or give dir a new folder for the React app".to_string());
            }
        }
    }
    let (written, kept) = write_files(starter, &folder)?;
    let install = match starter.kind {
        "python-api" => python_install(&folder),
        _ => crate::vite::npm_install(&folder),
    };
    let mut result = json!({
        "starter": starter.kind,
        "written": written.iter().map(|path| from_root(&root, &folder, path)).collect::<Vec<_>>(),
        "guide": starter.guide,
    });
    if !kept.is_empty() {
        result["kept"] = json!(kept
            .iter()
            .map(|path| from_root(&root, &folder, path))
            .collect::<Vec<_>>());
        result["kept_note"] =
            json!("These files were already in the project: Claw left them as they are.");
    }
    match install {
        Ok(()) => result["installed"] = json!(true),
        Err(error) => {
            result["installed"] = json!(false);
            result["install_error"] = json!(format!(
                "{error}. The libraries are not installed yet: run the install again (npm install, or pip in .venv) before checking the project."
            ));
        }
    }
    if starter.kind == "react-app" {
        result["check"] = json!(REACT_CHECK);
        if let Some(name) = designer_css(&folder) {
            result["design"] = json!(format!("{name} (the design conception's colors) is imported by src/index.css before src/theme.css: keep its colors, and leave src/theme.css as it is."));
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{find, react_index_css, starter_tool, write_files, StarterInput, STARTERS};
    use serde_json::json;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("claw-starter-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        dir
    }

    #[test]
    fn every_starter_is_found_by_its_kind_or_a_common_name() {
        assert_eq!(find("").map(|starter| starter.kind), Some("react-app"));
        assert_eq!(find("React").map(|starter| starter.kind), Some("react-app"));
        assert_eq!(
            find("express").map(|starter| starter.kind),
            Some("node-api")
        );
        assert_eq!(
            find("FastAPI").map(|starter| starter.kind),
            Some("python-api")
        );
        assert!(find("cobol").is_none());
        for starter in STARTERS {
            assert!(
                !starter.files.is_empty() && !starter.guide.is_empty(),
                "{}",
                starter.kind
            );
            assert!(
                starter.files.iter().any(|(path, _)| *path == ".gitignore"),
                "{}",
                starter.kind
            );
        }
    }

    #[test]
    fn the_react_starter_holds_the_kit_and_builds_into_docs() {
        let starter = find("react-app").expect("react");
        let file = |name: &str| {
            starter
                .files
                .iter()
                .find(|(path, _)| *path == name)
                .map_or_else(|| panic!("{name}"), |(_, text)| *text)
        };
        assert!(file("vite.config.ts").contains("outDir: \"docs\""));
        assert!(file("vite.config.ts").contains("base: \"./\""));
        assert!(file("src/index.css").contains("@import \"./kit/app-kit.css\" layer(components);"));
        assert!(file("src/main.tsx").contains("registerOffline"));
        assert!(file("index.html").contains("src=\"/src/main.tsx\""));
        let package: serde_json::Value = serde_json::from_str(file("package.json")).expect("json");
        for library in [
            "react",
            "framer-motion",
            "lucide-react",
            "zustand",
            "recharts",
            "radix-ui",
            "zod",
        ] {
            assert!(package["dependencies"].get(library).is_some(), "{library}");
        }
        for tool in ["vite", "typescript", "tailwindcss", "@tailwindcss/vite"] {
            assert!(package["devDependencies"].get(tool).is_some(), "{tool}");
        }
        assert_eq!(package["scripts"]["typecheck"], "tsc --noEmit");
        // Le kit exporte chaque fichier qu'il contient.
        let index = file("src/kit/index.ts");
        for (path, _) in starter
            .files
            .iter()
            .filter(|(path, _)| path.starts_with("src/kit/") && *path != "src/kit/index.ts")
        {
            let name = path
                .trim_start_matches("src/kit/")
                .split('.')
                .next()
                .unwrap_or_default();
            assert!(index.contains(&format!("from \"./{name}\"")), "{name}");
        }
    }

    #[test]
    fn the_designers_base_css_comes_before_the_apps_colors() {
        let dir = temp("base");
        let css = "@import \"tailwindcss\";\n@import \"./theme.css\";\n";
        assert_eq!(react_index_css(&dir, css), css);
        std::fs::write(dir.join("base.css"), ":root { --accent: #0a7; }").expect("base");
        assert_eq!(
            react_index_css(&dir, css),
            css,
            "le base.css de l'utilisateur n'est pas celui du concepteur"
        );
        std::fs::write(
            dir.join("base.css"),
            format!(
                "{}\n:root {{ --accent: #0a7; }}",
                runtime::web_design::BASE_CSS_MARK
            ),
        )
        .expect("base");
        assert_eq!(
            react_index_css(&dir, css),
            "@import \"tailwindcss\";\n@import \"../base.css\";\n@import \"./theme.css\";\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_second_call_rewrites_the_kit_and_keeps_the_models_work() {
        let dir = temp("again");
        let starter = find("react-app").expect("react");
        let (written, kept) = write_files(starter, &dir).expect("first");
        assert!(kept.is_empty());
        assert!(written.contains(&"src/kit/app-kit.css".to_string()));
        std::fs::write(dir.join("src/App.tsx"), "// l'appli du modèle").expect("app");
        std::fs::write(dir.join("src/kit/blocks.tsx"), "// abîmé").expect("kit");
        std::fs::write(dir.join(".gitignore"), "dist/").expect("ignore");
        let (written, kept) = write_files(starter, &dir).expect("second");
        assert_eq!(
            std::fs::read_to_string(dir.join("src/App.tsx")).expect("app"),
            "// l'appli du modèle"
        );
        assert!(kept.contains(&"src/App.tsx".to_string()));
        assert!(written.contains(&"src/kit/blocks.tsx".to_string()));
        assert!(std::fs::read_to_string(dir.join("src/kit/blocks.tsx"))
            .expect("kit")
            .contains("export function Hero"));
        let ignore = std::fs::read_to_string(dir.join(".gitignore")).expect("ignore");
        assert!(
            ignore.starts_with("dist/\n") && ignore.contains("node_modules/"),
            "{ignore}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Une appli dont chaque écran est un composant, avec son propre bouton +.
    const WRAPPED_SCREENS: &str = r#"import { useState } from "react";
import { House, NotebookPen } from "lucide-react";
import { AppBar, AppShell, Card, Screen, Sheet } from "./kit";

function Accueil() {
  return (
    <Screen id="accueil" label="Accueil">
      <Card title="Bienvenue">Premier écran</Card>
    </Screen>
  );
}

function Notes() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Screen id="notes" title="Notes" onAdd={() => setOpen(true)}>
        <Card title="Liste des notes">Aucune note</Card>
      </Screen>
      <Sheet open={open} onClose={() => setOpen(false)} title="Nouvelle note">
        <p>Texte de la note</p>
      </Sheet>
    </>
  );
}

export default function App() {
  const tabs = [
    { id: "accueil", label: "Accueil", icon: House },
    { id: "notes", label: "Notes", icon: NotebookPen },
  ];
  return (
    <AppShell bar={<AppBar title="Carnet" />} tabs={tabs} addLabel="Ajouter une note">
      <Accueil />
      <Notes />
    </AppShell>
  );
}
"#;

    #[test]
    #[ignore = "installe les bibliothèques (réseau) et lance le navigateur installé"]
    #[allow(clippy::too_many_lines)]
    fn the_react_starter_builds_and_works_in_a_real_browser() {
        // Décision du 28 octobre 2026 : React + Vite par défaut pour les applis.
        // Le projet de départ est posé par l'outil, ses bibliothèques installées,
        // puis la page est mesurée comme le fait la revue : Claw la construit
        // avant de l'ouvrir, et la reconstruit quand une source change.
        let name = format!("claw-react-{}", std::process::id());
        let dir = std::env::current_dir().expect("dossier").join(&name);
        let _ = std::fs::remove_dir_all(&dir);
        let input: StarterInput =
            serde_json::from_value(json!({ "kind": "react-app", "dir": name })).expect("entrée");
        let result = starter_tool(&input).expect("projet");
        assert_eq!(result["installed"], json!(true), "{result}");
        assert!(
            result["written"]
                .to_string()
                .contains(&format!("{name}/src/kit/app-kit.css")),
            "{result}"
        );
        let app = dir.join("src/App.tsx");
        let source = std::fs::read_to_string(&app).expect("App.tsx");
        let seeded = source
            .replace(
                "const SEED: NewEntry[] = [];",
                r#"const SEED: NewEntry[] = [
  { label: "Salaire", amount: 1450, category: "Autre" },
  { label: "Courses", amount: -62.4, category: "Courses" },
  { label: "Navigo", amount: -86.4, category: "Transport" },
];"#,
            )
            .replace(
                "persistentStore(\"budget\", { monthly: 0 })",
                "persistentStore(\"budget\", { monthly: 900 })",
            );
        assert_ne!(
            seeded, source,
            "l'exemple a changé : ses données de départ ne sont plus là"
        );
        std::fs::write(&app, &seeded).expect("App.tsx");
        let page = format!("{name}/index.html");
        let shots: Vec<_> = ["light", "dark"]
            .iter()
            .map(|theme| {
                let shot: crate::screenshot::ScreenshotInput = serde_json::from_value(json!({
                    "path": page,
                    "viewports": ["phone", "desktop"],
                    "theme": theme,
                    "windows": true,
                    "explore": true,
                    "figures": ["1450", "62.4", "86.4", "900"]
                }))
                .expect("entrée");
                crate::screenshot::take(&shot)
            })
            .collect();
        let built = dir.join("docs/index.html").is_file();
        let step = |input: serde_json::Value| {
            let input: crate::screenshot::browse::BrowserInput =
                serde_json::from_value(input).expect("entrée");
            crate::screenshot::browse::browse(&input)
                .unwrap_or_else(|error| json!({ "error": error }))
        };
        let journey = step(json!({ "action": "steps", "steps": [
            { "action": "open", "path": page, "expect": "Reste à dépenser", "absent": "NaN" },
            { "action": "click", "target": "[aria-label=\"Ajouter une opération\"]" },
            { "action": "type", "target": "Montant en euros", "text": "12,50" },
            { "action": "type", "target": "Libellé", "text": "Cinéma" },
            { "action": "click", "target": "dialog.sheet [type=submit]", "expect": "Dépense de 12,50 € enregistrée" },
            { "action": "click", "target": ".tabbar__tab[href=\"#operations\"]", "expect": "Cinéma", "absent": "Invalid Date" },
            { "action": "click", "target": "[aria-label=\"Modifier « Cinéma »\"]", "expect": "Modifier l'opération" },
            { "action": "click", "target": "Supprimer l'opération", "expect": "« Cinéma » supprimé" },
            { "action": "click", "target": "Annuler", "expect": "Cinéma" },
            { "action": "click", "target": ".tabbar__tab[href=\"#bilan\"]", "expect": "Rentrées et dépenses" }
        ] }));
        let _ = step(json!({ "action": "close" }));
        // Une source changée : la page est reconstruite avant d'être rouverte.
        std::fs::write(
            &app,
            seeded.replace("Reste à dépenser", "Disponible ce mois-ci"),
        )
        .expect("App.tsx");
        let view = step(json!({ "action": "steps", "steps": [
            { "action": "open", "path": page, "expect": "Disponible ce mois-ci" }
        ] }));
        let _ = step(json!({ "action": "close" }));
        // Des écrans écrits dans des composants à soi suivent les onglets, et le bouton + fait
        // ce que l'écran ouvert lui donne. Vu le 30 octobre 2026 (essai C5) : l'AppShell
        // montrait toujours le premier écran et le + ne pouvait pas ouvrir la fenêtre d'un écran.
        std::fs::write(&app, WRAPPED_SCREENS).expect("App.tsx");
        let wrapped = step(json!({ "action": "steps", "steps": [
            { "action": "open", "path": page, "expect": "Premier écran" },
            { "action": "click", "target": ".tabbar__tab[href=\"#notes\"]", "expect": "Liste des notes", "absent": "Premier écran" },
            { "action": "click", "target": ".tabbar__add", "expect": "Texte de la note" }
        ] }));
        let _ = step(json!({ "action": "close" }));
        // Une erreur de construction revient au modèle, avec sa cause.
        std::fs::write(
            dir.join("src/main.tsx"),
            "import App from \"./Absent\";\nconsole.log(App);\n",
        )
        .expect("main.tsx");
        let broken: crate::screenshot::ScreenshotInput =
            serde_json::from_value(json!({ "path": page })).expect("entrée");
        let broken = crate::screenshot::take(&broken);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(built, "docs/index.html");
        for shot in shots {
            let shot = shot.expect("captures");
            let text = shot.to_string();
            for defect in [
                "never shows",
                "typed text cannot be read",
                "Charts that show nothing",
                "do nothing when clicked",
                "cannot be tapped",
                "not in the project",
                "Invalid Date",
                "NaN",
                "wider than the screen",
                "too little contrast",
                "cannot be seen",
                "is shown nowhere",
                "cut off by the edge of a box",
                "Not reached",
            ] {
                assert!(!text.contains(defect), "{defect}: {text}");
            }
            for capture in shot["captures"].as_array().expect("captures") {
                let console = capture["console"].to_string();
                assert!(
                    !console.contains("uncaught") && !console.contains("error:"),
                    "{console}"
                );
            }
        }
        assert!(
            journey.get("failed").is_none() && journey.get("error").is_none(),
            "{journey}"
        );
        assert!(
            view.get("failed").is_none() && view.get("error").is_none(),
            "{view}"
        );
        assert!(
            wrapped.get("failed").is_none() && wrapped.get("error").is_none(),
            "{wrapped}"
        );
        let error = broken.expect_err("construction en échec");
        assert!(
            error.contains("did not build") && error.contains("Absent"),
            "{error}"
        );
    }
}
