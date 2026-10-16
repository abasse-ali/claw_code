// Client de l'API GitHub pour l'appli : dépôts, branches, sessions (issues du dépôt
// du moteur), lancement du workflow, suivi des passages, pull requests, secrets.

const API = "https://api.github.com";
export const WORKFLOW = "claw-web.yml";
export const LABEL = "claw-web";
export const ANSWER = "<!-- claw-web:answer -->";
export const PROGRESS = "<!-- claw-web:progress -->";

/** Chemin d'un fichier pour une adresse de l'API : chaque partie encodée, les « / » gardés. */
const encodePath = (path) => String(path || "").split("/").filter(Boolean).map(encodeURIComponent).join("/");

export class GitHubError extends Error {
  constructor(status, message) {
    super(message);
    this.status = status;
  }
}

export class GitHub {
  constructor(token) {
    this.token = token;
  }

  async request(method, path, body) {
    let response;
    try {
      response = await fetch(path.startsWith("https://") ? path : API + path, {
        method,
        cache: "no-store",
        headers: {
          Authorization: `Bearer ${this.token}`,
          Accept: "application/vnd.github+json",
          "X-GitHub-Api-Version": "2022-11-28",
          ...(body === undefined ? {} : { "Content-Type": "application/json" }),
        },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
    } catch {
      throw new GitHubError(0, "GitHub ne répond pas : vérifiez la connexion");
    }
    if (response.status === 204) return null;
    const text = await response.text();
    let data = null;
    try {
      data = text ? JSON.parse(text) : null;
    } catch {
      data = null;
    }
    if (!response.ok) {
      const reason = (data && data.message) || response.statusText;
      throw new GitHubError(response.status, explain(response.status, reason, path));
    }
    return data;
  }

  user() {
    return this.request("GET", "/user");
  }

  repository(fullName) {
    return this.request("GET", `/repos/${fullName}`);
  }

  /** Dépôts accessibles, les plus récemment poussés d'abord (jusqu'à 300). */
  async repositories() {
    const found = [];
    for (let page = 1; page <= 3; page += 1) {
      const batch = await this.request("GET",
        `/user/repos?per_page=100&page=${page}&sort=pushed&affiliation=owner,collaborator,organization_member`);
      found.push(...batch);
      if (batch.length < 100) break;
    }
    return found;
  }

  /** Nouveau dépôt du compte, avec un README (donc une branche `main` dès le départ). */
  createRepository({ name, description, isPrivate, gitignore }) {
    return this.request("POST", "/user/repos", {
      name,
      description: description || undefined,
      private: Boolean(isPrivate),
      auto_init: true,
      gitignore_template: gitignore || undefined,
    });
  }

  async branches(fullName) {
    const found = [];
    for (let page = 1; page <= 3; page += 1) {
      const batch = await this.request("GET", `/repos/${fullName}/branches?per_page=100&page=${page}`);
      found.push(...batch);
      if (batch.length < 100) break;
    }
    return found;
  }

  /** Sessions de l'appli : issues du moteur avec l'étiquette `claw-web`. */
  async sessions(engine) {
    const issues = await this.request("GET",
      `/repos/${engine}/issues?labels=${LABEL}&state=all&sort=updated&direction=desc&per_page=50`);
    return issues.filter((issue) => !issue.pull_request);
  }

  issue(engine, number) {
    return this.request("GET", `/repos/${engine}/issues/${number}`);
  }

  /** Commentaires d'une issue ; avec `since`, seulement ceux modifiés depuis (date ISO). */
  async comments(engine, number, since = "") {
    const found = [];
    const after = since ? `&since=${encodeURIComponent(since)}` : "";
    for (let page = 1; page <= 5; page += 1) {
      const batch = await this.request("GET", `/repos/${engine}/issues/${number}/comments?per_page=100&page=${page}${after}`);
      found.push(...batch);
      if (batch.length < 100) break;
    }
    return found;
  }

  createIssue(engine, title, body) {
    return this.request("POST", `/repos/${engine}/issues`, { title, body, labels: [LABEL] });
  }

  comment(engine, number, body) {
    return this.request("POST", `/repos/${engine}/issues/${number}/comments`, { body });
  }

  setIssueState(engine, number, state) {
    return this.request("PATCH", `/repos/${engine}/issues/${number}`, { state });
  }

  /** Lance un passage du moteur pour une session. */
  dispatch(engine, ref, inputs) {
    return this.request("POST", `/repos/${engine}/actions/workflows/${WORKFLOW}/dispatches`, { ref, inputs });
  }

  workflow(engine) {
    return this.request("GET", `/repos/${engine}/actions/workflows/${WORKFLOW}`);
  }

  async runs(engine) {
    const data = await this.request("GET", `/repos/${engine}/actions/workflows/${WORKFLOW}/runs?per_page=40`);
    return data.workflow_runs || [];
  }

  /** Requête GraphQL : ses erreurs arrivent avec un statut 200 ; elles deviennent des exceptions. */
  async graphql(query, variables) {
    const data = await this.request("POST", "/graphql", { query, variables });
    const errors = (data && data.errors) || [];
    if (errors.length) {
      const refused = errors.some((error) => error.type === "FORBIDDEN"
        || /not authorized|not accessible|permission/i.test(error.message || ""));
      throw new GitHubError(refused ? 403 : 422, errors.map((error) => error.message).join(" ; "));
    }
    return data ? data.data : null;
  }

  /** Efface une issue de GitHub, sans retour possible (réservé à qui administre le dépôt). */
  deleteIssue(nodeId) {
    return this.graphql("mutation($id: ID!) { deleteIssue(input: {issueId: $id}) { clientMutationId } }", { id: nodeId });
  }

  removeLabel(repo, number, label) {
    return this.request("DELETE", `/repos/${repo}/issues/${number}/labels/${encodeURIComponent(label)}`);
  }

  closePull(repo, number) {
    return this.request("PATCH", `/repos/${repo}/pulls/${number}`, { state: "closed" });
  }

  /** Supprime une branche ; déjà absente, rien à faire (faux). */
  async deleteBranch(repo, branch) {
    try {
      await this.request("DELETE", `/repos/${repo}/git/refs/heads/${branch}`);
      return true;
    } catch (error) {
      if (error instanceof GitHubError && (error.status === 404 || error.status === 422)) return false;
      throw error;
    }
  }

  deleteRun(engine, runId) {
    return this.request("DELETE", `/repos/${engine}/actions/runs/${runId}`);
  }

  /** Passages du moteur, jusqu'à `pages` pages de 100, les plus récents d'abord. */
  async allRuns(engine, pages = 3) {
    const runs = [];
    for (let page = 1; page <= pages; page += 1) {
      const data = await this.request("GET", `/repos/${engine}/actions/workflows/${WORKFLOW}/runs?per_page=100&page=${page}`);
      const batch = data.workflow_runs || [];
      runs.push(...batch);
      if (batch.length < 100) break;
    }
    return runs;
  }

  /** Crée la branche depuis `from` si elle n'existe pas encore. */
  async ensureBranch(repo, branch, from = "main") {
    if (await this.branchExists(repo, branch)) return;
    const base = await this.request("GET", `/repos/${repo}/git/ref/heads/${from}`);
    await this.request("POST", `/repos/${repo}/git/refs`, { ref: `refs/heads/${branch}`, sha: base.object.sha });
  }

  /** Nouvelle branche `name`, au même point que la branche `from`. */
  async createBranch(repo, name, from) {
    const base = await this.request("GET", `/repos/${repo}/git/ref/heads/${encodePath(from)}`);
    return this.request("POST", `/repos/${repo}/git/refs`, { ref: `refs/heads/${name}`, sha: base.object.sha });
  }

  /** Renomme une branche ; GitHub fait suivre les pull requests qui y arrivent et ses adresses. */
  renameBranch(repo, branch, name) {
    return this.request("POST", `/repos/${repo}/branches/${encodePath(branch)}/rename`, { new_name: name });
  }

  /** Dépose un fichier (contenu en base64) sur une branche. */
  putFile(repo, path, base64, message, branch) {
    return this.request("PUT", `/repos/${repo}/contents/${path}`, { message, content: base64, branch });
  }

  /** Contenu d'un dossier (liste) ou d'un fichier (objet) sur une branche. */
  contents(repo, path, ref) {
    return this.request("GET", `/repos/${repo}/contents/${encodePath(path)}?ref=${encodeURIComponent(ref)}`);
  }

  /** Contenu brut d'un fichier d'une branche, en Blob. */
  async rawFile(repo, path, branch) {
    let response;
    try {
      response = await fetch(`${API}/repos/${repo}/contents/${encodePath(path)}?ref=${encodeURIComponent(branch)}`, {
        cache: "force-cache",
        headers: { Authorization: `Bearer ${this.token}`, Accept: "application/vnd.github.raw", "X-GitHub-Api-Version": "2022-11-28" },
      });
    } catch {
      throw new GitHubError(0, "GitHub ne répond pas : vérifiez la connexion");
    }
    if (!response.ok) throw new GitHubError(response.status, `Image introuvable (${response.status})`);
    return response.blob();
  }

  /** Supprime un fichier d'une branche ; déjà absent, rien à faire. */
  async deleteFile(repo, path, message, branch) {
    try {
      const file = await this.request("GET", `/repos/${repo}/contents/${path}?ref=${encodeURIComponent(branch)}`);
      await this.request("DELETE", `/repos/${repo}/contents/${path}`, { message, sha: file.sha, branch });
    } catch (error) {
      if (!(error instanceof GitHubError) || error.status !== 404) throw error;
    }
  }

  /** La branche existe-t-elle dans le dépôt ? */
  async branchExists(repo, branch) {
    try {
      await this.request("GET", `/repos/${repo}/git/ref/heads/${branch}`);
      return true;
    } catch (error) {
      if (error instanceof GitHubError && error.status === 404) return false;
      throw error;
    }
  }

  setIssueBody(engine, number, body) {
    return this.request("PATCH", `/repos/${engine}/issues/${number}`, { body });
  }

  /** Différences entre deux branches : commits et fichiers, avec leurs différences. */
  compare(repo, base, head) {
    const ref = (name) => name.split("/").map(encodeURIComponent).join("/");
    return this.request("GET", `/repos/${repo}/compare/${ref(base)}...${ref(head)}`);
  }

  /** Arrête un passage : le moteur garde ce qui est fait sur la branche de la session. */
  cancelRun(engine, runId) {
    return this.request("POST", `/repos/${engine}/actions/runs/${runId}/cancel`);
  }

  async jobs(engine, runId) {
    const data = await this.request("GET", `/repos/${engine}/actions/runs/${runId}/jobs`);
    return data.jobs || [];
  }

  /** Fusionne une pull request dans sa branche de base. */
  mergePull(repo, number) {
    return this.request("PUT", `/repos/${repo}/pulls/${number}/merge`, { merge_method: "merge" });
  }

  /** Réglage GitHub Pages du dépôt, ou null s'il n'est pas publié. */
  async pages(repo) {
    try {
      return await this.request("GET", `/repos/${repo}/pages`);
    } catch (error) {
      if (error instanceof GitHubError && error.status === 404) return null;
      throw error;
    }
  }

  /** Publie le dépôt sur GitHub Pages depuis une branche et un dossier (`/` ou `/docs`). */
  async publishPages(repo, branch, path) {
    const source = { source: { branch, path } };
    if (await this.pages(repo)) {
      await this.request("PUT", `/repos/${repo}/pages`, source);
      return this.pages(repo);
    }
    return this.request("POST", `/repos/${repo}/pages`, source);
  }

  /** Le fichier existe-t-il sur cette branche ? */
  async fileExists(repo, path, ref) {
    try {
      await this.request("GET", `/repos/${repo}/contents/${path}?ref=${encodeURIComponent(ref)}`);
      return true;
    } catch (error) {
      if (error instanceof GitHubError && error.status === 404) return false;
      throw error;
    }
  }

  setVisibility(repo, isPrivate) {
    return this.request("PATCH", `/repos/${repo}`, { private: isPrivate });
  }

  async pull(fullName, branch) {
    const owner = fullName.split("/")[0];
    const pulls = await this.request("GET",
      `/repos/${fullName}/pulls?state=all&head=${encodeURIComponent(`${owner}:${branch}`)}&per_page=5`);
    return pulls[0] || null;
  }

  secretKey(engine) {
    return this.request("GET", `/repos/${engine}/actions/secrets/public-key`);
  }

  putSecret(engine, name, encrypted, keyId) {
    return this.request("PUT", `/repos/${engine}/actions/secrets/${name}`, { encrypted_value: encrypted, key_id: keyId });
  }

  async hasSecret(engine, name) {
    try {
      await this.request("GET", `/repos/${engine}/actions/secrets/${name}`);
      return true;
    } catch (error) {
      if (error.status === 404) return false;
      throw error;
    }
  }

  async ensureLabel(engine) {
    try {
      await this.request("POST", `/repos/${engine}/labels`,
        { name: LABEL, color: "E8743B", description: "Session de l'appli web de Claw" });
    } catch (error) {
      if (error.status !== 422) throw error;
    }
  }
}

/** Message d'erreur compréhensible pour l'utilisateur. */
function explain(status, reason, path) {
  if (status === 401) return "Jeton GitHub refusé (invalide ou expiré) : renouvelez-le dans les réglages";
  if (status === 403 && /rate limit/i.test(reason)) return "Limite de requêtes GitHub atteinte : réessayez dans quelques minutes";
  if (path === "/user/repos" && status === 422) {
    return /already exists/i.test(reason) ? "Vous avez déjà un dépôt de ce nom" : `Création refusée : ${reason}`;
  }
  if (/\/pages$/.test(path)) {
    if (status === 422 && /plan|private/i.test(reason)) {
      return "GitHub Pages refuse ce dépôt privé (compte gratuit) : rendez-le public pour le publier gratuitement";
    }
    if (status === 403 || status === 404) {
      return "Le jeton n'a pas les autorisations que GitHub demande pour activer Pages : dans ses réglages sur GitHub (Repository permissions), Administration et Pages en lecture et écriture (la valeur du jeton ne change pas)";
    }
    return `GitHub Pages : ${reason}`;
  }
  if (/\/merge$/.test(path)) {
    if (status === 405 || status === 409) return `Fusion impossible : ${reason} (conflit ou vérifications en cours ; voyez la pull request sur GitHub)`;
  }
  if (/\/rename$/.test(path)) {
    if (status === 403 || status === 404) {
      return "Le jeton ne peut pas renommer cette branche : pour la branche par défaut d'un dépôt, GitHub demande l'autorisation Administration en lecture et écriture (réglages du jeton sur GitHub ; sa valeur ne change pas)";
    }
    if (status === 422) return `Renommage refusé : ${reason}`;
  }
  if (/\/git\/refs$/.test(path) && status === 422) {
    return /already exists/i.test(reason) ? "Une branche porte déjà ce nom" : `Branche refusée : ${reason}`;
  }
  if (status === 403 || status === 404) {
    if (path === "/user/repos") {
      return "Le jeton ne peut pas créer de dépôt : dans ses réglages sur GitHub, choisissez « All repositories » et l'autorisation Administration en lecture et écriture (le jeton garde la même valeur)";
    }
    if (path.includes("/secrets")) return "Le jeton n'a pas le droit de gérer les secrets du moteur (autorisation Secrets)";
    if (path.includes("/dispatches") || path.includes("/actions/")) return "Le jeton n'a pas accès aux Actions du moteur (autorisation Actions)";
    return `Accès refusé ou introuvable (${reason}) : vérifiez les dépôts et autorisations du jeton`;
  }
  return `GitHub : ${reason} (${status})`;
}
