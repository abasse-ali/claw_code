# E3. Erreurs des fournisseurs et du flux

Question : pour chaque erreur qui vient d'un fournisseur de modèles ou du flux de réponse
(limite de débit, quota, surcharge, contexte trop long, réponse vide, appel d'outil coupé ou mal
formé, clé refusée, modèle introuvable), que fait Claw, qui est prévenu, et la reprise est-elle
sérieuse ?

Étude menée par la session principale. Même forme de constat que E1.

Ce qui était déjà traité avant l'étude, pour mémoire : limite par minute attendue puis réessayée,
quota du jour non réessayé, panne en cours de réponse relancée sur le même modèle ou un autre
avec l'accord de l'utilisateur, `stream_options` refusé retiré, outils refusés remplacés par le
mode texte, appel d'outil coupé redemandé, réponse vide relancée une fois, réflexion qui prend
toute la sortie ramenée d'un cran.

## E3-01 Un paramètre facultatif refusé arrête le tour
- Gravité : haute
- Où : rust/crates/api/src/providers/openai_compat.rs, `send_negotiated` et `send_raw_request`
- Aujourd'hui : avec un niveau de réflexion réglé (`/think`, ultracode), Claw envoie
  `reasoning_effort` à tout serveur compatible. Un serveur ou un modèle qui ne le connaît pas
  répond 400 et le tour s'arrête sur « l'api a retourné 400 ». Seul `stream_options` avait une
  reprise.
- Défaut : l'utilisateur doit deviner qu'il faut couper la réflexion pour ce modèle.
- Remède : une erreur 400 ou 422 qui nomme `reasoning_effort`, `reasoning`,
  `parallel_tool_calls` ou `tool_choice` fait repartir la même requête sans ce champ ; le refus
  est retenu pour ce serveur et ce modèle. Au plus quatre refus par requête.
- État : corrigé (lot 1)

## E3-02 Un schéma d'outil refusé arrête le tour
- Gravité : haute
- Où : même fichier, `plain_schema`, `is_schema_refusal`
- Aujourd'hui : les schémas des outils portent `additionalProperties`, `format`, `minItems`,
  `minimum`, `default`. L'API de Google refuse un mot-clé inconnu par « Unknown name … at
  'tools[0].function_declarations[0].parameters' ». Rien n'était prévu.
- Défaut : aucun outil ne marche avec ce serveur, sans explication utile.
- Remède : quand l'erreur vise le schéma, la requête repart avec des schémas réduits à
  l'essentiel (types, propriétés, éléments, valeurs permises, descriptions), et ce serveur les
  reçoit ainsi jusqu'à la fin. Une propriété dont le nom ne commence pas par une lettre (`-A`
  de `grep_search`) est renommée (`A`).
- État : corrigé côté envoi (lot 1). Reste à faire accepter par `grep_search` les noms
  `A`, `B`, `C`, `n`, `i` (voir A10 du suivi). Non vérifié sur le vrai Gemini (A8).

## E3-03 Identifiants d'appels d'un autre fournisseur refusés par Mistral
- Gravité : moyenne
- Où : même fichier, `mistral_call_ids`
- Aujourd'hui : une session commencée sur OpenRouter garde des identifiants comme
  `call-47d07cf6-be75-…`. L'API de Mistral n'accepte que neuf lettres ou chiffres.
- Défaut : après un changement de modèle vers Mistral, chaque requête est refusée.
- Remède : pour `api.mistral.ai`, chaque identifiant qui n'a pas cette forme est remplacé par
  une empreinte de neuf caractères, la même pour l'appel et pour son résultat.
- État : corrigé (lot 1). La contrainte vient de l'expérience de l'API, elle ne figure
  pas dans la spécification fournie : non vérifié sur le vrai serveur.

## E3-04 Contexte trop long : plusieurs formulations ne sont pas reconnues
- Gravité : moyenne
- Où : rust/crates/runtime/src/conversation.rs, `is_context_overflow`
- Aujourd'hui : sont reconnus « context » avec « length », « window » ou « too long »,
  « prompt is too long », « maximum context », « too many tokens ».
- Défaut : ne sont pas reconnus « exceeds the maximum number of tokens » (Google),
  « Request too large for model » et le statut 413 (Groq, limite de jetons par minute),
  « reduce the length of the messages » (Cerebras), « token limit ». Le tour s'arrête alors
  qu'une compaction suffirait.
- Remède : élargir la reconnaissance, y compris le statut 413.
- État : corrigé (lot 1). Sont aussi reconnus « maximum number of tokens », « request too
  large », « reduce the length », « token limit », « input is too long » et le statut 413 : la
  conversation est compactée et la requête repart une fois.

## E3-05 Une réponse en texte coupée par la limite de sortie est rendue telle quelle
- Gravité : moyenne
- Où : rust/crates/claw-cli/src/main.rs (`check_cut_calls`), rust/crates/runtime/src/conversation.rs
- Aujourd'hui : un appel d'outil coupé par la limite de sortie est redemandé. Une réponse en
  texte coupée au milieu d'une phrase termine le tour sans rien dire.
- Défaut : l'utilisateur reçoit un rapport tronqué, et un contrôle de fin de tour peut porter
  sur un texte incomplet.
- Remède : quand la réponse sans appel d'outil s'arrête sur la limite de sortie, Claw demande
  la suite (« continue exactly where you stopped »), une ou deux fois au plus, et le dit.
- État : à faire

## E3-06 Les erreurs 400, 413 et 422 arrivent à l'utilisateur avec le conseil « claw --help »
- Gravité : basse
- Où : rust/crates/claw-cli/src/main.rs, `error_hint`
- Aujourd'hui : seuls 429, 401, les politiques de données, les images et les pannes ont un
  conseil.
- Défaut : une requête refusée pour son contenu ne dit pas quoi essayer.
- Remède : un conseil par famille (requête refusée par le fournisseur : changer de modèle ou
  réduire la conversation avec `/compact` ; crédits insuffisants ; modèle introuvable).
- État : corrigé (lot 1). `error_hint` lit le code HTTP (`returned_status`) et donne un conseil
  pour 402, 403, 404, 413 et le contexte trop long, 400 et 422, le serveur injoignable ; par
  défaut « renvoyez la demande ; si l'erreur revient, changez de modèle (/model) ou lancez
  `claw doctor` ».
