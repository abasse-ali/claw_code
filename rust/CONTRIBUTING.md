# Contribution

Merci de contribuer à Claw Code.

## Mise en place de l'environnement de développement

- Installez la chaîne d'outils Rust stable.
- Travaillez depuis la racine du dépôt dans cet espace de travail Rust. Si vous êtes parti de la racine du dépôt parent, faites d'abord `cd rust/`.

## Build

```bash
cargo build
cargo build --release
```

## Tester et vérifier

Lancez l'ensemble complet de vérifications Rust avant d'ouvrir une pull request :

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace
cargo test --workspace
```

Si vous modifiez le comportement, ajoutez ou mettez à jour les tests pertinents dans la même pull request.

## Style de code

- Suivez les motifs existants dans le crate modifié au lieu d'introduire un nouveau style.
- Formatez le code avec `rustfmt`.
- Gardez `clippy` propre pour les cibles d'espace de travail que vous avez modifiées.
- Préférez des diffs ciblés aux refactorisations opportunistes.

## Pull requests

- Branchez depuis `main`.
- Cantonnez chaque pull request à un seul changement clair.
- Expliquez la motivation, le résumé de l'implémentation et la vérification que vous avez exécutée.
- Assurez-vous que les vérifications locales passent avant de demander une revue.
- Si les retours de revue modifient le comportement, relancez les commandes de vérification pertinentes.
