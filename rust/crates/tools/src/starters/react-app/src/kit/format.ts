// Montants et dates de l'appli, dans la langue de la page (fr) et en euros.

const lang = document.documentElement.lang || "fr";
const currency = document.documentElement.dataset.currency || "EUR";
const moneyFormat = new Intl.NumberFormat(lang, { style: "currency", currency });
const signedFormat = new Intl.NumberFormat(lang, { style: "currency", currency, signDisplay: "exceptZero" });
const dayFormat = new Intl.DateTimeFormat(lang, { day: "numeric", month: "short" });
const monthFormat = new Intl.DateTimeFormat(lang, { month: "long", year: "numeric" });
const shortMonthFormat = new Intl.DateTimeFormat(lang, { month: "short" });

const pad = (value: number) => String(value).padStart(2, "0");

/** « 1 235,15 € » pour 1235.15 : des euros, pas des centimes (divisez-les par 100 avant). */
export const money = (value: number) => moneyFormat.format(Number.isFinite(value) ? value : 0);

/** « +100,00 € », « −8,00 € » : des euros, comme money(). */
export const signed = (value: number) => signedFormat.format(Number.isFinite(value) ? value : 0);

/** « 1 235,15 € », « 1235.15 », « -8 », « 12,5 » : un nombre, ou NaN. Le dernier séparateur
 * suivi d'un ou deux chiffres est celui des décimales, les autres sont des milliers. */
export function parseAmount(input: string | number): number {
  if (typeof input === "number") return input;
  const text = String(input ?? "").replace(/€|\beur(os?)?\b/gi, "");
  if (/\p{L}/u.test(text)) return NaN;
  const digits = text.replace(/[^\d,.+-]/g, "");
  const last = Math.max(digits.lastIndexOf(","), digits.lastIndexOf("."));
  const decimals = last >= 0 && /^\d{1,2}$/.test(digits.slice(last + 1));
  const whole = (decimals ? digits.slice(0, last) : digits).replace(/[,.]/g, "");
  const value = Number(decimals ? `${whole}.${digits.slice(last + 1)}` : whole);
  return digits && Number.isFinite(value) ? value : NaN;
}

/** Aujourd'hui, « 2026-10-28 ». */
export function today(): string {
  const now = new Date();
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}

/** Le mois d'une date, « 2026-10 ». */
export const monthOf = (date: string) => date.slice(0, 7);

/** Le mois `step` mois plus tard (ou plus tôt). */
export function shiftMonth(month: string, step: number): string {
  const [year, number] = month.split("-").map(Number);
  const date = new Date(year, number - 1 + step, 1);
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}`;
}

/** « Octobre 2026 ». */
export function monthName(month: string): string {
  const [year, number] = month.split("-").map(Number);
  const text = monthFormat.format(new Date(year, number - 1, 1));
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/** « oct. ». */
export function shortMonth(month: string): string {
  const [year, number] = month.split("-").map(Number);
  return shortMonthFormat.format(new Date(year, number - 1, 1));
}

/** « 28 oct. ». */
export function dayLabel(date: string): string {
  const [year, month, day] = date.split("-").map(Number);
  return dayFormat.format(new Date(year, month - 1, day));
}

/** Jours qui restent dans le mois, aujourd'hui compris. */
export function daysLeft(month = monthOf(today())): number {
  const [year, number] = month.split("-").map(Number);
  const last = new Date(year, number, 0).getDate();
  const now = new Date();
  const current = now.getFullYear() === year && now.getMonth() + 1 === number;
  return current ? last - now.getDate() + 1 : last;
}
