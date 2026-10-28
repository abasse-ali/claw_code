// Mon budget : les rentrées et les charges de chaque mois (réglables), et les
// opérations du mois gardées par le kit (window.entries).
(() => {
  const PLAN_KEY = "budget-plan";
  const startPlan = {
    incomes: [
      { id: "salaire", label: "Salaire", min: 1235.15, max: 1235.15, sure: true, category: "Salaire" },
      { id: "mobili", label: "Mobili-Jeune", min: 100, max: 100, sure: true, category: "Aides" },
      { id: "caf", label: "CAF", min: 103, max: 206, sure: true, category: "CAF" },
      { id: "especes", label: "Espèces", min: 80, max: 200, sure: false, category: "Espèces" },
    ],
    charges: [
      { id: "loyer", label: "Loyer", amount: 450, category: "Logement" },
      { id: "forfait", label: "Forfait", amount: 10, category: "Forfait" },
      { id: "claude", label: "Claude", amount: 21.6, category: "Abonnement" },
      { id: "transport", label: "Transport", amount: 8, note: "16 € remboursés à 50 % par Orange", category: "Transport" },
      { id: "alimentaire", label: "Alimentaire", amount: 100, category: "Courses" },
    ],
  };
  const thisMonth = () => window.entries.today().slice(0, 7);
  // Le plan vaut à partir du mois où l'appli a été ouverte la première fois :
  // les mois d'avant restent vides, jamais inventés.
  const load = () => {
    try {
      const stored = JSON.parse(localStorage.getItem(PLAN_KEY));
      if (stored && stored.incomes) return stored;
    } catch (_) { /* plan illisible : celui de départ */ }
    return { ...structuredClone(startPlan), start: thisMonth() };
  };
  let plan = load();
  const save = () => { try { localStorage.setItem(PLAN_KEY, JSON.stringify(plan)); } catch (_) { /* rien à faire */ } };
  save();
  const money = (value) => window.entries.format(value);
  // « 103 à 206 € » : les centimes seulement quand il y en a.
  const short = (value) => (Number.isInteger(value) ? new Intl.NumberFormat("fr", { style: "currency", currency: "EUR", maximumFractionDigits: 0 }).format(value) : money(value));
  const range = (item) => (item.min === item.max ? money(item.min) : `${short(item.min).replace(/\s?€/, "")} à ${short(item.max)}`);
  const show = (name, text) => document.querySelectorAll(`[data-show="${name}"]`).forEach((node) => { node.textContent = text; });

  const figures = () => {
    const month = window.entries.totals(window.entries.month());
    const sure = plan.incomes.filter((item) => item.sure).reduce((sum, item) => sum + item.min, 0);
    const sureBest = plan.incomes.filter((item) => item.sure).reduce((sum, item) => sum + item.max, 0);
    const unsureMin = plan.incomes.filter((item) => !item.sure).reduce((sum, item) => sum + item.min, 0);
    const unsureMax = plan.incomes.filter((item) => !item.sure).reduce((sum, item) => sum + item.max, 0);
    const fixed = plan.charges.reduce((sum, item) => sum + item.amount, 0);
    const prudent = sure - fixed + month.income - month.spending;
    return {
      sure, fixed, month, prudent,
      habit: prudent + unsureMin,
      best: sureBest + unsureMax - fixed + month.income - month.spending,
      budget: sure - fixed + month.income,
      unsure: unsureMax,
    };
  };

  const planRow = (item, kind) => {
    const row = document.createElement("li");
    row.className = "row";
    const income = kind === "incomes";
    const amount = income ? range(item) : money(item.amount);
    row.innerHTML = `<span class="row__icon ${income ? "row__icon--in" : ""}">${window.entries.iconFor(item.category, item.label)}</span><span class="row__main"><span class="row__title"><span class="row__text"></span></span><span class="row__sub"></span></span><span class="row__end"><span class="row__amount"></span></span>`;
    const title = row.querySelector(".row__title");
    title.querySelector(".row__text").textContent = item.label;
    // Une pastille par ligne au plus : le nom reste entier.
    if (income && !item.sure) title.insertAdjacentHTML("beforeend", '<span class="badge badge--warn">pas sûr</span>');
    else if (income && item.min !== item.max) title.insertAdjacentHTML("beforeend", '<span class="badge badge--accent">variable</span>');
    row.querySelector(".row__sub").textContent = item.note || (income ? (item.sure ? "Chaque mois" : "Compté une fois reçu") : "Chaque mois");
    const end = row.querySelector(".row__amount");
    end.textContent = income ? `+${amount}` : `−${amount}`;
    end.classList.toggle("row__amount--in", income);
    return row;
  };

  const settingRow = (item, kind, field) => {
    const row = document.createElement("div");
    row.className = "setting";
    const id = `reglage-${item.id}-${field}`;
    row.innerHTML = `<label class="setting__name" for="${id}"></label><div class="money"><input id="${id}" inputmode="decimal" autocomplete="off"><span aria-hidden="true">€</span></div>`;
    const label = row.querySelector("label");
    label.textContent = item.label;
    if (field === "max") label.insertAdjacentHTML("beforeend", "<small>au plus</small>");
    else if (kind === "incomes" && item.min !== item.max) label.insertAdjacentHTML("beforeend", "<small>au moins</small>");
    const input = row.querySelector("input");
    input.value = String(item[field]).replace(".", ",");
    input.addEventListener("change", () => {
      const value = window.entries.parseAmount(input.value);
      if (!(value >= 0)) { input.value = String(item[field]).replace(".", ","); window.toast("Un montant comme 12,50.", { tone: "error" }); return; }
      item[field] = Math.round(value * 100) / 100;
      if (kind === "incomes" && item.min > item.max) item.max = item.min;
      save();
      render();
      drawBudget();
      window.toast(`${item.label} : ${money(item[field])}`);
    });
    return row;
  };

  const render = () => {
    const f = figures();
    const now = new Date();
    const thisMonth = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}`;
    const daysInMonth = new Date(now.getFullYear(), now.getMonth() + 1, 0).getDate();
    const daysLeft = window.app.month === thisMonth ? daysInMonth - now.getDate() + 1 : daysInMonth;
    show("left", money(f.prudent));
    show("best", money(f.best));
    show("per-day", money(Math.max(0, f.prudent) / daysLeft));
    show("days-left", `${daysLeft} jour${daysLeft > 1 ? "s" : ""} restant${daysLeft > 1 ? "s" : ""}`);
    const used = f.budget > 0 ? Math.min(100, Math.round((f.month.spending / f.budget) * 100)) : 0;
    show("used", `${used} % du budget dépensé`);
    show("budget", money(f.budget));
    const bar = document.querySelector('[data-show="used-bar"]');
    if (bar) { bar.style.setProperty("--value", `${used}%`); bar.parentElement.setAttribute("aria-valuenow", String(used)); }
    show("sure", money(f.sure));
    show("unsure", `+ jusqu'à ${money(f.unsure)} non assurés`);
    show("fixed", money(f.fixed));
    show("fixed-count", `${plan.charges.length} charges chaque mois`);
    show("incomes-count", `${plan.incomes.length} prévues`);
    show("charges-count", `${money(f.fixed)} par mois`);
    show("habit", money(f.habit));
    show("year", money(f.habit * 12));
    for (const list of document.querySelectorAll("[data-plan]")) {
      list.replaceChildren(...plan[list.dataset.plan].map((item) => planRow(item, list.dataset.plan)));
    }
    const scenarios = document.querySelector('[data-show="scenarios"]');
    if (scenarios) {
      const top = Math.max(f.best, 1);
      const meter = (name, value, note) => `<div class="meter"><div class="meter__head"><span class="meter__name">${name}</span><span class="meter__value">${money(value)}</span></div><div class="meter__track"><i style="--value: ${Math.max(0, Math.round((value / top) * 100))}%"></i></div><p class="meter__note">${note}</p></div>`;
      scenarios.innerHTML = meter("Prudent", f.prudent, "CAF au plus bas, sans les espèces") + meter("Habituel", f.habit, "avec les espèces au plus bas") + meter("Au mieux", f.best, "CAF et espèces au plus haut");
    }
  };

  const renderSettings = () => {
    const incomes = document.querySelector('[data-settings="incomes"]');
    if (incomes) {
      incomes.replaceChildren(...plan.incomes.flatMap((item) => (item.min === item.max && item.sure ? [settingRow(item, "incomes", "min")] : [settingRow(item, "incomes", "min"), settingRow(item, "incomes", "max")])));
    }
    const charges = document.querySelector('[data-settings="charges"]');
    if (charges) charges.replaceChildren(...plan.charges.map((item) => settingRow(item, "charges", "amount")));
  };

  // Le graphique du bilan : chaque mois depuis le premier, les rentrées sûres
  // et les charges du plan, plus les opérations du mois.
  const chart = document.querySelector('[data-chart="budget"]');
  const drawBudget = () => {
    if (!chart) return;
    const months = [];
    for (let back = 5; back >= 0; back -= 1) {
      const [year, month] = window.app.month.split("-").map(Number);
      const date = new Date(year, month - 1 - back, 1);
      months.push(`${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}`);
    }
    const sure = plan.incomes.filter((item) => item.sure).reduce((sum, item) => sum + item.min, 0);
    const fixed = plan.charges.reduce((sum, item) => sum + item.amount, 0);
    const label = new Intl.DateTimeFormat("fr", { month: "short" });
    const sums = months.map((month) => {
      const own = window.entries.totals(window.entries.month(month));
      const planned = month >= plan.start;
      return { income: (planned ? sure : 0) + own.income, spending: (planned ? fixed : 0) + own.spending };
    });
    window.drawChart(chart, {
      labels: months.map((month) => label.format(new Date(`${month}-01T12:00:00`)).replace(".", "")),
      series: [
        { name: "Rentrées", tone: "in", values: sums.map((sum) => sum.income) },
        { name: "Dépenses", tone: "out", values: sums.map((sum) => sum.spending) },
      ],
    });
  };
  document.addEventListener("entries:change", drawBudget);
  document.addEventListener("monthchange", drawBudget);
  document.addEventListener("screenchange", () => requestAnimationFrame(drawBudget));
  document.addEventListener("entries:change", render);
  document.addEventListener("monthchange", render);
  renderSettings();
  render();
  drawBudget();
})();
