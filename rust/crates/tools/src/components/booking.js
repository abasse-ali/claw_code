// Réservation : les jours des deux semaines à venir qui ont des créneaux, les
// places restantes, la confirmation, et un fichier .ics pour l'agenda. Sans
// serveur, les réservations restent dans ce navigateur ; avec
// data-endpoint="https://..." sur la section, chacune y est envoyée en JSON.
(() => {
  for (const root of document.querySelectorAll("[data-booking]")) {
    if (root.dataset.ready) continue;
    root.dataset.ready = "1";
    let slots = [];
    try {
      slots = JSON.parse(root.querySelector("[data-booking-slots]").textContent);
    } catch (_) {
      slots = [];
    }
    const lang = document.documentElement.lang || "fr";
    const days = root.querySelector("[data-booking-days]");
    const list = root.querySelector("[data-booking-slots-list]");
    const form = root.querySelector("[data-booking-form]");
    const summary = root.querySelector("[data-booking-summary]");
    const done = root.querySelector("[data-booking-done]");
    const doneText = root.querySelector("[data-booking-done-text]");
    const minutes = Number(root.dataset.duration || 60);
    const dayName = new Intl.DateTimeFormat(lang, { weekday: "short", day: "numeric", month: "short" });
    const longDay = new Intl.DateTimeFormat(lang, { weekday: "long", day: "numeric", month: "long" });
    const pad = (value) => String(value).padStart(2, "0");
    const isoDay = (date) => `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
    const key = (date, slot) => `booking:${isoDay(date)}:${slot.time}:${slot.label}`;
    const taken = (date, slot) => Number(localStorage.getItem(key(date, slot)) || 0);
    let chosen = null;

    const press = (container, button) => {
      for (const other of container.querySelectorAll("button")) other.setAttribute("aria-pressed", String(other === button));
    };
    const showSlots = (date) => {
      list.replaceChildren();
      for (const slot of slots.filter((item) => item.day === date.getDay())) {
        const left = Math.max(0, slot.places - taken(date, slot));
        const button = document.createElement("button");
        button.type = "button";
        button.className = "booking__slot";
        button.setAttribute("aria-pressed", "false");
        button.disabled = left === 0;
        button.innerHTML = `<strong>${slot.time}</strong><span>${slot.label}</span><small>${left === 0 ? "complet" : `${left} place${left > 1 ? "s" : ""}`}</small>`;
        button.addEventListener("click", () => {
          press(list, button);
          chosen = { date, slot };
          summary.textContent = `${slot.label}, ${longDay.format(date)} à ${slot.time}.`;
          form.hidden = false;
          done.hidden = true;
          form.querySelector("input").focus();
        });
        list.append(button);
      }
    };
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    for (let offset = 0; offset < 14; offset += 1) {
      const date = new Date(today);
      date.setDate(today.getDate() + offset);
      if (!slots.some((slot) => slot.day === date.getDay())) continue;
      const button = document.createElement("button");
      button.type = "button";
      button.className = "booking__day";
      button.setAttribute("aria-pressed", "false");
      button.textContent = dayName.format(date);
      button.addEventListener("click", () => {
        press(days, button);
        chosen = null;
        form.hidden = true;
        showSlots(date);
      });
      days.append(button);
    }

    const stamp = (date, time, extra = 0) => {
      const [hours, mins] = time.split(":").map(Number);
      const start = new Date(date);
      start.setHours(hours, mins + extra, 0, 0);
      return `${start.getFullYear()}${pad(start.getMonth() + 1)}${pad(start.getDate())}T${pad(start.getHours())}${pad(start.getMinutes())}00`;
    };
    let last = null;
    form.addEventListener("submit", async (event) => {
      event.preventDefault();
      if (!chosen || !form.reportValidity()) return;
      const button = form.querySelector('[type="submit"]');
      button.disabled = true;
      const data = Object.fromEntries(new FormData(form));
      const booking = { ...data, date: isoDay(chosen.date), time: chosen.slot.time, label: chosen.slot.label };
      try {
        if (root.dataset.endpoint) {
          const response = await fetch(root.dataset.endpoint, { method: "POST", headers: { "Content-Type": "application/json", Accept: "application/json" }, body: JSON.stringify(booking) });
          if (!response.ok) throw new Error(String(response.status));
        }
        localStorage.setItem(key(chosen.date, chosen.slot), String(taken(chosen.date, chosen.slot) + 1));
        last = chosen;
        doneText.textContent = `Réservé : ${summary.textContent}`;
        form.hidden = true;
        form.reset();
        done.hidden = false;
        showSlots(chosen.date);
        if (typeof window.toast === "function") window.toast("Réservation confirmée");
      } catch (_) {
        if (typeof window.toast === "function") window.toast("La réservation n'a pas pu être envoyée, réessayez.", { tone: "error" });
      } finally {
        button.disabled = false;
      }
    });
    root.querySelector("[data-booking-ics]").addEventListener("click", () => {
      if (!last) return;
      const lines = ["BEGIN:VCALENDAR", "VERSION:2.0", "PRODID:-//site//reservation//FR", "BEGIN:VEVENT",
        `UID:${Date.now()}@reservation`, `DTSTAMP:${stamp(new Date(), "00:00")}`,
        `DTSTART:${stamp(last.date, last.slot.time)}`, `DTEND:${stamp(last.date, last.slot.time, minutes)}`,
        `SUMMARY:${last.slot.label}`, `LOCATION:${root.dataset.location || ""}`, "END:VEVENT", "END:VCALENDAR"];
      const link = document.createElement("a");
      link.href = URL.createObjectURL(new Blob([lines.join("\r\n")], { type: "text/calendar" }));
      link.download = "reservation.ics";
      link.click();
      setTimeout(() => URL.revokeObjectURL(link.href), 1000);
    });
  }
})();
