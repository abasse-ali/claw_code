// Suivis jour par jour (habitudes, objectifs) : les cases des derniers jours, et les séries et le
// taux qui s'en déduisent. Les jours sont des dates « 2026-10-28 » à l'heure du téléphone
// (today(), shiftDay(), lastDays()), jamais tirées de toISOString(), qui donne la veille entre
// minuit et 1 h ou 2 h en France (essai D1 de Claw Web, 30 octobre 2026).

import { Check } from "lucide-react";
import { lastDays, longDayLabel, shiftDay, today, weekdayLabel } from "./format";
import { Icon } from "./icons";

/** Les cases des derniers jours, du plus ancien à aujourd'hui (« lun. 26 »), cochées ou non d'un
 * toucher ; `label` (le nom de l'habitude) les nomme pour les lecteurs d'écran. */
export function WeekChecks({ done, onToggle, label, days = lastDays(7) }: {
  done: string[]; onToggle: (day: string) => void; label: string; days?: string[];
}) {
  const now = today();
  return (
    <div className="week" role="group" aria-label={label}>
      {days.map((day) => {
        const checked = done.includes(day);
        const [weekday, number] = weekdayLabel(day).split(" ");
        return (
          <button
            key={day}
            type="button"
            className={`week__day${day === now ? " is-today" : ""}`}
            aria-pressed={checked}
            aria-label={`${label}, ${longDayLabel(day)}${day === now ? " (aujourd'hui)" : ""}`}
            onClick={() => onToggle(day)}
          >
            <span className="week__name">{weekday}</span>
            <span className="week__mark">{checked ? <Icon icon={Check} size={18} /> : number}</span>
          </button>
        );
      })}
    </div>
  );
}

/** La série en cours : les jours cochés à la suite jusqu'à aujourd'hui, ou jusqu'à hier tant
 * qu'aujourd'hui n'est pas coché. */
export function currentStreak(done: string[], until = today()): number {
  const days = new Set(done);
  let day = days.has(until) ? until : shiftDay(until, -1);
  let count = 0;
  while (days.has(day)) {
    count += 1;
    day = shiftDay(day, -1);
  }
  return count;
}

/** La plus longue suite de jours cochés. */
export function bestStreak(done: string[]): number {
  const days = [...new Set(done)].sort();
  let best = 0;
  let run = 0;
  days.forEach((day, index) => {
    run = index > 0 && shiftDay(days[index - 1], 1) === day ? run + 1 : 1;
    best = Math.max(best, run);
  });
  return best;
}

/** La part des `days` cochés, en pourcentage arrondi à l'unité (3 sur 7 : 43). */
export function successRate(done: string[], days = lastDays(7)): number {
  if (!days.length) return 0;
  const set = new Set(done);
  return Math.round((days.filter((day) => set.has(day)).length / days.length) * 100);
}
