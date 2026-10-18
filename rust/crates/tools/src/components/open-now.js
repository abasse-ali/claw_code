// « Ouvert maintenant » d'après les horaires, à l'heure de l'appareil : quand
// le lieu ferme, ou quand il rouvre ; la ligne du jour est marquée. Mis à
// jour chaque minute.
(() => {
  const names = ["dimanche", "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi"];
  const say = (time) => {
    const [hours, minutes] = time.split(":");
    return `${Number(hours)} h${minutes === "00" ? "" : ` ${minutes}`}`;
  };
  const toMinutes = (time) => { const [hours, minutes] = time.split(":").map(Number); return hours * 60 + minutes; };
  for (const root of document.querySelectorAll("[data-hours]")) {
    if (root.dataset.ready) continue;
    root.dataset.ready = "1";
    let hours = {};
    try { hours = JSON.parse(root.querySelector("[data-hours-data]").textContent); } catch (_) { hours = {}; }
    const status = root.querySelector("[data-hours-status]");
    const update = () => {
      const now = new Date();
      const today = now.getDay();
      const minute = now.getHours() * 60 + now.getMinutes();
      for (const row of root.querySelectorAll("[data-hours-day]")) row.classList.toggle("is-today", Number(row.dataset.hoursDay) === today);
      const open = (hours[today] || []).find(([from, to]) => minute >= toMinutes(from) && minute < toMinutes(to));
      if (open) {
        status.dataset.open = "";
        status.textContent = `Ouvert maintenant · ferme à ${say(open[1])}`;
        return;
      }
      delete status.dataset.open;
      const later = (hours[today] || []).find(([from]) => toMinutes(from) > minute);
      if (later) { status.textContent = `Fermé · ouvre à ${say(later[0])}`; return; }
      for (let ahead = 1; ahead <= 7; ahead += 1) {
        const day = (today + ahead) % 7;
        const first = (hours[day] || [])[0];
        if (first) {
          status.textContent = `Fermé · ouvre ${ahead === 1 ? "demain" : names[day]} à ${say(first[0])}`;
          return;
        }
      }
      status.textContent = "Fermé";
    };
    update();
    setInterval(update, 60000);
  }
})();
