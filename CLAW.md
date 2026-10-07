# CLAW.md

Ce fichier fournit des indications à Claw Code lors du travail sur le code de ce dépôt.

## Stack détectée
- Langages : Rust.
- Frameworks : aucun détecté à partir des marqueurs de starter pris en charge.

## Vérification
- Lancer la vérification Rust depuis `rust/` : `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`
- `src/` et `tests/` sont tous deux présents ; mettre à jour les deux surfaces ensemble lorsque le comportement change.

## Structure du dépôt
- `rust/` contient l'espace de travail Rust et l'implémentation CLI/runtime active.
- `src/` contient les fichiers source qui doivent rester cohérents avec les indications générées et les tests.
- `tests/` contient les surfaces de validation qui doivent être revues en même temps que les changements de code.

## Accord de travail
- Préférer de petits changements revisables et garder les fichiers d'amorçage générés alignés sur les workflows réels du dépôt.
- Conserver les valeurs par défaut partagées dans `.claw.json` ; réserver `.claw/settings.local.json` aux surcharges locales à la machine.
- Ne pas écraser automatiquement le contenu existant de `CLAW.md` ; le mettre à jour intentionnellement lorsque les workflows du dépôt changent.
