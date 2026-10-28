// Le journal en mots simples : les annonces du moteur (écrites pour le suivi de Claw, avec des
// « (s) », des noms complets de modèles et des mots de métier) redites comme on les dirait à
// l'utilisateur. Vaut aussi pour les journaux déjà publiés : rien ne change dans le moteur.

/** Nom lisible d'un modèle : « nvidia:nvidia/nemotron-3-ultra-550b-a55b » devient
 * « Nemotron 3 Ultra », « poolside/laguna-s-2.1:free » « Laguna S 2.1 ». */
export function modelName(id) {
  const name = String(id || "").replace(/:free$/, "").split(/[/:]/).pop()
    .replace(/-\d+(?:\.\d+)?b(?:-a\d+(?:\.\d+)?b)?$/i, "")
    .replace(/-(?:it|instruct|preview|latest)$/i, "");
  return name.split("-").filter(Boolean).map((word) => {
    if (/^(?:glm|vl|gpt|lfm|ai)$/i.test(word)) return word.toUpperCase();
    if (/^v?\d/.test(word)) return word;
    return word.charAt(0).toUpperCase() + word.slice(1);
  }).join(" ") || String(id || "");
}

// Un identifiant de modèle dans une phrase : « fournisseur:propriétaire/nom » ou
// « propriétaire/nom:free » (pas un chemin de fichier, qui n'a ni l'un ni l'autre).
const MODEL = /\b(?:nvidia|gemini|ollama|openrouter):[\w.-]+(?:\/[\w.-]+)?(?::free)?|\b[a-z][\w.-]*\/[\w.-]+:free\b/gi;

/** Les « (s) » et « (x) » d'une phrase accordés au nombre qui les précède dans la même partie
 * (« 1 défaut(s) mesuré(s) » : « 1 défaut mesuré » ; sans nombre : au pluriel). */
function agreed(text) {
  return text.split(/([:;.]\s)/).map((part) => {
    let count = null;
    return part.replace(/(\d+(?:[,.]\d+)?)|([\p{L}'’-]+)\((e?s|x)\)/gu, (whole, number, word, mark) => {
      if (number !== undefined) {
        count = Number(number.replace(",", "."));
        return whole;
      }
      const many = count === null || count > 1;
      return many ? word + mark : mark === "es" ? `${word}e` : word;
    });
  }).join("");
}

/** La dernière note d'une suite (« 6,4 puis 6,6 puis 6,6/10 » : « 6,6/10 »). */
const lastGrade = (text) => {
  const grades = String(text).match(/(\d+(?:,\d+)?)(?=\s*(?:puis|\/10))/g) || [];
  return grades.length ? `${grades[grades.length - 1]}/10` : "";
};

const plural = (count, one, many = `${one}s`) => `${count} ${Number(count) > 1 ? many : one}`;

/** Chaque annonce connue, et sa phrase simple. L'ordre compte : la première qui correspond. */
const RULES = [
  [/^Claw vérifie la page après la modification : (\d+) défaut\(s\) nouveau\(x\), (\d+) réparé\(s\), (\d+) restant\(s\)/,
    (m) => checkPhrase(Number(m[1]), Number(m[2]), Number(m[3]))],
  [/^Mesure de la page : (\d+) défaut\(s\) à reprendre/, (m) => `Page mesurée : ${plural(m[1], "défaut")} à corriger, envoyés à l'agent`],
  [/^Claw mesure la page : (\d+) modifications de l'interface sans la regarder/, (m) => `${m[1]} modifications sans regarder la page : Claw la mesure`],
  [/^Claw mesure la page avant la conclusion/, () => "Claw mesure la page avant de conclure, pour dire ce qui reste"],
  [/^(\S+) ne lit pas les images : description de (.+?) demandée(?: en arrière-plan)? à un modèle qui les lit(?: \((\S+?)\))?/,
    (m) => `${modelName(m[1])} ne voit pas les images : ${m[3] ? modelName(m[3]) : "un autre modèle"} lui décrit ${m[2]}`],
  [/^(\S+) ne lit pas les images : il reçoit à la place la description de (.+?) par (\S+?)\.?$/,
    (m) => `${modelName(m[1])} ne voit pas les images : ${modelName(m[3])} lui décrit ${m[2]}`],
  [/^(\S+) ne lit pas les images : (\d+ images?) n'(?:a|ont) pas pu lui être décrite?s?/,
    (m) => `${modelName(m[1])} ne voit pas les images, et ${m[2]} n'a pas pu lui être décrite`],
  [/^Revue visuelle : un modèle qui voit note les captures/, () => "Un modèle qui voit les images note le design"],
  [/^Revue visuelle de l'interface \((\d+)\/(\d+)\)/, (m) => `Revue du design (${m[1]} sur ${m[2]}) : Claw prend des captures au téléphone et à l'ordinateur`],
  [/^Revue visuelle impossible : .*ERR_CONNECTION_REFUSED/, () => "Revue du design impossible : la page ne s'ouvre pas (serveur arrêté ?)"],
  [/^Revue visuelle sautée : il reste peu de requêtes/, () => "Revue du design sautée : il reste peu de requêtes gratuites aujourd'hui"],
  [/^Revue visuelle : (\d+) défaut\(s\) absent\(s\) au début du tour, introduit\(s\) par ce tour/,
    (m) => `Revue du design : ${plural(m[1], "défaut créé", "défauts créés")} pendant ce passage, à réparer d'abord`],
  [/^Revue visuelle : rien à reprendre sur la page(.*)$/, (m) => `Revue du design : rien à reprendre${m[1]}`],
  [/^Revue visuelle : (.+?) : renvoyé\(s\) à l'agent$/, (m) => `Revue du design : ${m[1]}, envoyés à l'agent`],
  [/^Revue visuelle reportée : la page appelle (.+?), pas encore écrit/, (m) => `Revue du design reportée : ${m[1]} pas encore écrit`],
  [/^Revue visuelle : (.+)$/, (m) => `Revue du design : ${m[1]}`],
  [/^Design (?:arrêté|bloqué) sous la barre (\d+) \((.+?)\) : Claw demande au juge des changements de structure/,
    (m) => `Design noté ${lastGrade(m[2])} (objectif ${m[1]}) : Claw demande des changements plus profonds, le travail continue`],
  [/^(Design|Fonctionnement) arrêté sous la barre (\d+) \((.+?)\)/,
    (m) => `${m[1]} noté ${lastGrade(m[3])} (objectif ${m[2]}) : la note ne monte plus, Claw s'arrête là. Écrivez « continue » pour reprendre`],
  [/^Poursuite du design \(\d+\/\d+, limite du tour\) : note (.+?\/10), sous la barre (\d+)/,
    (m) => `Design noté ${lastGrade(m[1])} (objectif ${m[2]}) : l'agent continue de l'améliorer`],
  [/^Fonctionnement (.+?\/10), sous la barre (\d+)/, (m) => `Fonctionnement noté ${lastGrade(m[1])} (objectif ${m[2]}) : l'agent continue de le réparer`],
  [/^Le tour a utilisé (\d+) requêtes sur (\d+) sans revue visuelle/, (m) => `${m[1]} requêtes sur ${m[2]} sans regarder le design : Claw le fait relire maintenant`],
  [/^Revue du plan de design par un autre modèle/, () => "Un autre modèle relit le plan de design, avant le code"],
  [/^Revue du plan par (\S+) : (\d+) remarque/, (m) => `Plan de design relu par ${modelName(m[1])} : ${plural(m[2], "remarque")} pour l'agent`],
  [/^Pas de plan de design avant le code/, () => "L'agent code sans plan de design : Claw lui en demande un"],
  [/^Wireframe du plan mal aligné/, () => "Maquette mal alignée : Claw la fait redessiner"],
  [/^Conception du design par (\S+) : plan et feuille de base \(([^,]+), \d+ lignes\)/,
    (m) => `${modelName(m[1])} a conçu le design (le plan et ${m[2]}) : l'agent construit tout le reste`],
  [/^Conception du design par (\S+) : plan/, (m) => `${modelName(m[1])} a conçu le plan du design : l'agent construit tout le reste`],
  [/^Conception du design : aucun modèle de conception n'a répondu/, () => "Aucun modèle de design n'a répondu : l'agent conçoit le design lui-même"],
  [/^(\S+) du modèle de conception remis en place/, (m) => `${m[1]} remis comme le concepteur l'a écrit : l'agent met ses règles dans sa propre feuille`],
  [/^État de la page avant ce tour : Claw la mesure/, () => "Claw mesure la page avant de commencer, pour voir ensuite ce que le travail casserait"],
  [/^État de la page avant ce tour : (\d+) défaut/, (m) => `Avant le travail, la page a déjà ${plural(m[1], "défaut")}`],
  [/^État de la page avant ce tour : transmis à l'agent/, () => "Claw donne cet état de la page à l'agent"],
  [/^Première version de la page écrite/, () => "Première version de la page : Claw la fait comparer à la demande avant les retouches"],
  [/^Claw fait confronter le travail à chaque exigence/, () => "Avant de conclure, Claw fait vérifier chaque point de la demande"],
  [/^Claw recadre le modèle : (\d+) appels d'outils dans ce tour/, (m) => `Claw recadre l'agent : déjà ${m[1]} actions dans ce passage`],
  [/^Claw recadre le modèle : deux modifications de (.+) ratées/, (m) => `Claw recadre l'agent : deux modifications de ${m[1]} n'ont pas marché`],
  [/^Claw recadre le modèle : (\d+) échecs d'affilée/, (m) => `Claw recadre l'agent : ${m[1]} échecs de suite`],
  [/^Claw recadre le modèle : (\d+) appels identiques à (\S+) sans effet/, (m) => `Claw recadre l'agent : ${m[1]} fois la même action sans effet`],
  [/^Claw recadre le modèle : (.+)$/, (m) => `Claw recadre l'agent : ${m[1]}`],
  [/^Contexte compacté automatiquement : (\d+) message/, (m) => `Conversation trop longue : Claw résume ${plural(m[1], "ancien message", "anciens messages")} pour continuer`],
  [/^Équipe : « (.+?) » confié à (\S+)/, (m) => `Équipe : « ${m[1]} » confié à ${modelName(m[2])}`],
  [/^(\S+) indisponible \((erreur \d+)\) → bascule sur (\S+)/, (m) => `${modelName(m[1])} refusé (${m[2]}) : Claw passe à ${modelName(m[3])}`],
  [/^(\S+) a cessé de répondre \((erreur \d+)\) → la requête repart sur (\S+)/, (m) => `${modelName(m[1])} ne répond plus (${m[2]}) : ${
    modelName(m[1]) === modelName(m[3]) ? "Claw le rappelle chez un autre fournisseur" : `Claw passe à ${modelName(m[3])}`}`],
  [/^(\S+) : quota ou limite de requêtes atteinte \(erreur 429\)\. Nouvel essai automatique dans (\d+ s) \((essai \d+ sur \d+)\)/,
    (m) => `${modelName(m[1])} est saturé : nouvel essai dans ${m[2]} (${m[3]})`],
  [/^(\S+) invente encore des messages de l'utilisateur : la requête repart sur (\S+)/,
    (m) => `${modelName(m[1])} invente des messages de votre part : Claw passe à ${modelName(m[2])}`],
  [/^Le modèle a écrit un faux message de l'utilisateur/, () => "L'agent a inventé un message de votre part : réponse écartée, nouvel essai"],
  [/^Le modèle n'a rien répondu deux fois : (\S+) reprend la fin du tour/, (m) => `L'agent n'a rien répondu deux fois : ${modelName(m[1])} finit le travail`],
  [/^La réflexion de (\S+) a pris toute la place de sa réponse/, (m) => `${modelName(m[1])} a réfléchi trop longtemps sans répondre : Claw raccourcit sa réflexion`],
  [/^Aucune vérification automatique connue pour ce projet/, () => "Pas de test connu pour ce projet : Claw ne peut pas vérifier les modifications tout seul"],
  [/^Vérification automatique après modification : (.+)$/, (m) => `Claw lance les tests : ${m[1]}`],
  [/^Erreur de syntaxe dans (.+?) : signalée au modèle/, (m) => `Erreur de syntaxe dans ${m[1]} : Claw la signale à l'agent`],
  [/^Pièges relevés dans (.+?) : (\d+), signalés au modèle/,
    (m) => `${plural(m[2], "piège")} dans ${m[1]} : Claw ${Number(m[2]) > 1 ? "les" : "le"} signale à l'agent`],
  [/^Icône\(s\) dessinée\(s\) à la main dans (.+?) : (\d+)/,
    (m) => `${plural(m[2], "icône dessinée", "icônes dessinées")} à la main dans ${m[1]} : Claw demande de prendre ${Number(m[2]) > 1 ? "celles" : "celle"} du jeu d'icônes`],
  [/^Emoji dans (.+?) : à remplacer par du SVG/, (m) => `Emoji dans ${m[1]} : Claw demande à l'agent de le remplacer par une icône`],
  [/^L'agent n'avance plus : Claw demande un diagnostic à un autre modèle/, () => "L'agent n'avance plus : Claw demande l'avis d'un autre modèle"],
  [/^Diagnostic de (\S+) transmis à l'agent/, (m) => `Avis de ${modelName(m[1])} transmis à l'agent`],
  [/^Relecture des modifications par un autre modèle/, () => "Un autre modèle relit les modifications"],
  [/^Relecture par (\S+) : (.+)$/, (m) => `Relecture par ${modelName(m[1])} : ${m[2]}`],
  [/^(?:Ce|Le) tour a utilisé (\d+) requêtes, la limite d'un tour[^:]*:/,
    (m) => `${m[1]} requêtes, le maximum d'un passage : Claw demande à l'agent de conclure. Écrivez « continue » pour poursuivre`],
  [/^Aucun autre modèle n'a répondu \(.*\) : (.+)$/, (m) => `Aucun autre modèle n'a répondu : ${m[1]}`],
  [/^Tâches pas finies \((\d+) restantes?\) : le tour continue, (\d+) requêtes de plus/,
    (m) => `${plural(m[1], "tâche pas finie", "tâches pas finies")} : Claw donne ${m[2]} requêtes de plus à l'agent`],
  [/^L'agent n'a pas conclu : le tour s'arrête là/, () => "L'agent n'a pas conclu : le passage s'arrête là"],
  [/^Questions posées à l'utilisateur/, () => "Claw vous pose des questions : il attend vos réponses"],
  [/^Réponse coupée par la limite de sortie du modèle/, () => "Réponse trop longue, coupée : Claw demande la suite"],
  [/^La suite de la réponse coupée n'est pas venue/, () => "La suite de la réponse coupée n'est pas venue : elle est montrée telle quelle"],
];

/** « Page vérifiée » : ce qui a changé, puis ce qui reste. */
function checkPhrase(fresh, fixed, left) {
  const changes = [fixed ? plural(fixed, "défaut réparé", "défauts réparés") : "",
    fresh ? plural(fresh, "nouveau défaut", "nouveaux défauts") : ""].filter(Boolean);
  const rest = left ? `${plural(left, "défaut")} à corriger` : "aucun défaut";
  return `Page vérifiée : ${changes.length ? `${changes.join(", ")}, ` : ""}${changes.length && left ? `encore ${rest}` : rest}`;
}

/** Une annonce du moteur, en mots simples : { text, kind } ; `kind` vaut « check » pour une
 * vérification de la page (avec `left`, les défauts qui restent, et `changed`), « warn » pour un
 * ennui (refus, erreur, arrêt), « info » sinon. */
export function plainNotice(raw) {
  const original = String(raw || "").trim();
  let text = original;
  for (const [pattern, phrase] of RULES) {
    const found = original.match(pattern);
    if (found) {
      text = phrase(found);
      break;
    }
  }
  text = agreed(text)
    .replace(MODEL, (id) => modelName(id))
    .replace(/\bClaw recadre le modèle\b/g, "Claw recadre l'agent")
    .replace(/\b(signalée?s?|transmise?s?|renvoyée?s?) au modèle\b/g, "$1 à l'agent")
    .replace(/\ble modèle de la session\b/g, "l'agent")
    .replace(/`([^`]+)`/g, "$1")
    // Le chemin complet d'un fichier sur la machine du passage : celui du projet suffit.
    .replace(/\/home\/runner\/\S*?\/work\//g, "")
    // La liste des critères du juge (en anglais) et les noms des réglages : pas pour l'utilisateur.
    .replace(/\(barre (\d+)[^)]*\)/g, "(objectif $1)")
    .replace(/\s*\((?:escalationModel|turnRequestLimit|designModel)\)/g, "")
    .replace(/\b(le|ce|du|au|un|d'un|chaque) tour\b/g, "$1 passage")
    .replace(/…$/, "");
  const check = original.match(/^Claw vérifie la page après la modification : (\d+) défaut\(s\) nouveau\(x\), (\d+) réparé\(s\), (\d+) restant/);
  if (check) return { text, kind: "check", left: Number(check[3]), changed: Number(check[1]) + Number(check[2]) > 0 };
  const warn = /erreur|refusé|impossible|ne répond plus|n'avance plus|saturé|coupée|arrêté|s'arrête|invent|échec|n'ont pas marché/i.test(text);
  return { text, kind: warn ? "warn" : "info" };
}

// Quelques mots anglais très courants : une réflexion qui en est pleine est en anglais.
const ENGLISH = /\b(?:the|and|to|of|is|it|this|that|with|for|need|let|now|should|user|wants|I'll|I|we|have|be|on)\b/gi;
const FRENCH = /\b(?:le|la|les|et|de|des|du|un|une|je|il|pour|est|que|qui|dans|sur|avec)\b/gi;

/** La réflexion d'un modèle est en anglais (le moteur ne la traduit pas). */
export function looksEnglish(text) {
  const sample = String(text || "").slice(0, 600);
  return (sample.match(ENGLISH) || []).length > (sample.match(FRENCH) || []).length + 1;
}
