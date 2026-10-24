"""Expressions cron à 5 champs (minute heure jour mois jour-de-la-semaine) des routines,
comme celles de Claude Code : heure UTC, ou celle d'un fuseau avec le préfixe
`CRON_TZ=Europe/Paris `. Valeurs, listes (`1,15`), plages (`1-5`), pas (`*/2`, `8-18/2`),
noms des mois (`JAN`) et des jours (`MON`) ; dimanche vaut 0 ou 7.
"""

import re
from datetime import datetime, timedelta, timezone

try:
    from zoneinfo import ZoneInfo, ZoneInfoNotFoundError
except ImportError:  # Python < 3.9
    ZoneInfo, ZoneInfoNotFoundError = None, Exception

FIELDS = (("minute", 0, 59), ("heure", 0, 23), ("jour", 1, 31), ("mois", 1, 12), ("jour de la semaine", 0, 7))
MONTHS = {name: index for index, name in enumerate(
    ("JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC"), 1)}
DAYS = {name: index for index, name in enumerate(("SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"))}


class CronError(ValueError):
    """Expression refusée ; le message dit pourquoi (en anglais : il va au modèle)."""


class Cron:
    def __init__(self, expression):
        self.expression = (expression or "").strip()
        text = self.expression
        self.zone = timezone.utc
        found = re.match(r"CRON_TZ=(\S+)\s+", text)
        if found:
            if ZoneInfo is None:
                raise CronError("time zones are not available on this machine: write the fields in UTC")
            try:
                self.zone = ZoneInfo(found.group(1))
            except (ZoneInfoNotFoundError, ValueError):
                raise CronError(f"unknown time zone {found.group(1)!r} (use an IANA name such as Europe/Paris)") from None
            text = text[found.end():]
        parts = text.split()
        if len(parts) != 5:
            raise CronError("a cron expression has 5 fields: minute hour day-of-month month day-of-week")
        self.minutes, self.hours, self.days, self.months, self.weekdays = (
            parse_field(part, *spec, names=MONTHS if index == 3 else DAYS if index == 4 else None)
            for index, (part, spec) in enumerate(zip(parts, FIELDS)))
        if 7 in self.weekdays:
            self.weekdays = (self.weekdays - {7}) | {0}
        # Règle de cron : quand jour du mois et jour de la semaine sont tous deux restreints,
        # l'un ou l'autre suffit.
        self.any_day = parts[2] == "*"
        self.any_weekday = parts[4] == "*"

    def matches(self, moment):
        local = moment.astimezone(self.zone)
        if local.minute not in self.minutes or local.hour not in self.hours or local.month not in self.months:
            return False
        return self._day_matches(local)

    def _day_matches(self, local):
        day = local.day in self.days
        weekday = (local.isoweekday() % 7) in self.weekdays
        if self.any_day and self.any_weekday:
            return True
        if self.any_day:
            return weekday
        if self.any_weekday:
            return day
        return day or weekday

    def next_after(self, moment, limit_days=400):
        """Le premier instant (minute pleine, UTC) strictement après `moment`, ou None."""
        local = (moment.astimezone(self.zone) + timedelta(minutes=1)).replace(second=0, microsecond=0)
        end = local + timedelta(days=limit_days)
        while local < end:
            if local.month not in self.months or not self._day_matches(local):
                local = (local + timedelta(days=1)).replace(hour=0, minute=0)
                continue
            if local.hour not in self.hours:
                local = (local + timedelta(hours=1)).replace(minute=0)
                continue
            if local.minute not in self.minutes:
                local += timedelta(minutes=1)
                continue
            # Fuseau : une heure qui n'existe pas (passage à l'heure d'été) est sautée.
            utc = local.astimezone(timezone.utc)
            if utc.astimezone(self.zone).replace(tzinfo=None) == local.replace(tzinfo=None):
                return utc
            local += timedelta(minutes=1)
        return None

    def fires_more_than_hourly(self):
        """Vrai si l'expression peut se déclencher plus d'une fois dans une même heure."""
        return len(self.minutes) > 1

    def recurring(self):
        return True


def parse_field(text, label, low, high, names=None):
    values = set()
    for item in text.split(","):
        if not item:
            raise CronError(f"empty value in the {english(label)} field")
        step = 1
        if "/" in item:
            item, _, step_text = item.partition("/")
            if not step_text.isdigit() or int(step_text) < 1:
                raise CronError(f"invalid step {step_text!r} in the {english(label)} field")
            step = int(step_text)
        if item == "*":
            start, end = low, high
        elif "-" in item:
            first, _, last = item.partition("-")
            start, end = value_of(first, label, low, high, names), value_of(last, label, low, high, names)
            if start > end:
                raise CronError(f"range {item!r} goes backwards in the {english(label)} field")
        else:
            start = value_of(item, label, low, high, names)
            end = high if step > 1 else start
        values.update(range(start, end + 1, step))
    return values


def value_of(text, label, low, high, names):
    key = text.upper()
    if names and key in names:
        return names[key]
    if not text.isdigit():
        raise CronError(f"invalid value {text!r} in the {english(label)} field")
    value = int(text)
    if not low <= value <= high:
        raise CronError(f"{value} is out of range {low}-{high} in the {english(label)} field")
    return value


def english(label):
    return {"minute": "minute", "heure": "hour", "jour": "day-of-month", "mois": "month",
            "jour de la semaine": "day-of-week"}[label]


def parse_time(text):
    """Un horodatage RFC 3339 (« 2026-10-24T08:00:00Z », « …+02:00 ») en UTC, à la minute."""
    try:
        moment = datetime.fromisoformat(str(text).strip().replace("Z", "+00:00"))
    except ValueError:
        raise CronError(f"invalid timestamp {text!r}: use RFC 3339, e.g. 2026-10-24T08:00:00Z") from None
    if moment.tzinfo is None:
        raise CronError(f"timestamp {text!r} has no time zone: add Z (UTC) or an offset such as +02:00")
    return moment.astimezone(timezone.utc).replace(second=0, microsecond=0)


def iso(moment):
    return moment.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ") if moment else None
