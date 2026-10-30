// Les blocs des écrans : la carte du chiffre principal (Hero), les tuiles, les cartes, les
// listes de lignes, les badges, les jauges et l'état vide. Leur allure vient de app-kit.css.

import { animate, AnimatePresence, motion, useMotionValue, useReducedMotion, useTransform } from "framer-motion";
import { useEffect, useId, type CSSProperties, type ReactNode } from "react";
import { money, signed } from "./format";
import { Icon, type LucideIcon } from "./icons";

export type Tone = "in" | "out" | "accent" | "warn";

/** Un montant en euros qui défile jusqu'à sa nouvelle valeur quand il change. */
export function Amount({ value, format = money }: { value: number; format?: (value: number) => string }) {
  const motionValue = useMotionValue(value);
  const text = useTransform(motionValue, (current) => format(current));
  const still = useReducedMotion();
  useEffect(() => {
    if (still) {
      motionValue.set(value);
      return;
    }
    const controls = animate(motionValue, value, { duration: 0.5, ease: "easeOut" });
    return () => controls.stop();
  }, [value, still, motionValue]);
  return <motion.span>{text}</motion.span>;
}

type Pill = { label: ReactNode; value: ReactNode; note?: ReactNode };
type HeroProps = {
  label: ReactNode;
  /** Le chiffre principal, en euros ; ou `value` pour autre chose qu'un montant (« 12 jours »). */
  amount?: number;
  value?: ReactNode;
  sub?: ReactNode;
  /** Deux ou trois petits chiffres sous le chiffre principal. */
  pills?: Pill[];
  /** Une barre de progression : `value` de 0 à 1, `label` lu à voix haute, la légende dessous. */
  progress?: { value: number; label: string; start?: ReactNode; end?: ReactNode };
};

/** La carte du chiffre principal, sur le dégradé de l'appli. */
export function Hero({ label, amount, value, sub, pills, progress }: HeroProps) {
  const percent = progress ? Math.round(Math.min(1, Math.max(0, progress.value || 0)) * 100) : 0;
  return (
    <div className="hero">
      <p className="hero__label">{label}</p>
      <p className="hero__amount">{amount !== undefined ? <Amount value={amount} /> : value}</p>
      {sub && <p className="hero__sub">{sub}</p>}
      {pills && pills.length > 0 && (
        <div className="hero__pills">
          {pills.map((pill, index) => (
            <div className="hero__pill" key={index}>
              <span>{pill.label}</span>
              <b>{pill.value}</b>
              {pill.note && <small>{pill.note}</small>}
            </div>
          ))}
        </div>
      )}
      {progress && (
        <>
          <div className="hero__bar" role="progressbar" aria-label={progress.label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent}>
            <i style={{ "--value": `${percent}%` } as CSSProperties} />
          </div>
          {(progress.start || progress.end) && (
            <div className="hero__legend"><span>{progress.start}</span><span>{progress.end}</span></div>
          )}
        </>
      )}
    </div>
  );
}

/** Deux tuiles côte à côte (des <Tile>). */
export function Tiles({ children }: { children: ReactNode }) {
  return <div className="tiles">{children}</div>;
}

/** Un chiffre clé : son nom (avec une icône), sa valeur, une note dessous. */
export function Tile({ icon, label, value, note }: { icon?: LucideIcon; label: ReactNode; value: ReactNode; note?: ReactNode }) {
  return (
    <div className="tile">
      <p className="tile__label">{icon && <Icon icon={icon} size={16} />}{label}</p>
      <p className="tile__value">{value}</p>
      {note && <p className="tile__note">{note}</p>}
    </div>
  );
}

type CardProps = {
  title?: ReactNode;
  /** Un petit texte à droite du titre (« 3 charges »). */
  meta?: ReactNode;
  /** Un lien à droite du titre (« Tout voir » vers #operations). */
  link?: { label: string; href?: string; onClick?: () => void };
  level?: 2 | 3;
  className?: string;
  children?: ReactNode;
};

/** Une carte : un titre, puis une liste, des jauges, un texte ou un graphique. */
export function Card({ title, meta, link, level = 2, className, children }: CardProps) {
  const id = useId();
  const Heading = level === 3 ? "h3" : "h2";
  return (
    <section className={`card${className ? ` ${className}` : ""}`} aria-labelledby={title ? id : undefined}>
      {(title || meta || link) && (
        <div className="card__head">
          {title && <Heading id={id}>{title}</Heading>}
          {meta && <span className="card__meta">{meta}</span>}
          {link && (link.href ? (
            <a className="card__link" href={link.href}>{link.label}</a>
          ) : (
            <button className="card__link" type="button" onClick={link.onClick}>{link.label}</button>
          ))}
        </div>
      )}
      {children}
    </section>
  );
}

/** Une liste de lignes (des <Row> avec chacune sa `key`) : les lignes ajoutées entrent en glissant, les lignes retirées s'effacent. */
export function List({ children, label }: { children: ReactNode; label?: string }) {
  return (
    <ul className="list" aria-label={label}>
      <AnimatePresence initial={false}>{children}</AnimatePresence>
    </ul>
  );
}

type RowProps = {
  icon?: LucideIcon;
  /** La couleur de la pastille de l'icône. */
  tone?: Tone;
  title: ReactNode;
  badge?: ReactNode;
  sub?: ReactNode;
  /** Un montant à droite en euros (12.5, pas 1250 centimes), signé (+100,00 €, −8,00 €), vert
   * quand il est positif. */
  amount?: number;
  /** Autre chose à droite (un badge, un bouton), sous le montant s'il y en a un, avec une petite
   * note dessous. */
  end?: ReactNode;
  endNote?: ReactNode;
  /** Un bouton au bout de la ligne (modifier, supprimer). */
  action?: { icon: LucideIcon; label: string; onClick: () => void };
  done?: boolean;
};

/** Une ligne de liste : pastille d'icône, titre et détail, montant à droite. */
export function Row({ icon, tone = "accent", title, badge, sub, amount, end, endNote, action, done }: RowProps) {
  return (
    <motion.li
      layout="position"
      className={`row${done ? " is-done" : ""}`}
      initial={{ opacity: 0, y: 6 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, transition: { duration: 0.15 } }}
      transition={{ duration: 0.25, ease: "easeOut" }}
    >
      {icon && <span className={`row__icon row__icon--${tone}`}><Icon icon={icon} /></span>}
      <span className="row__main">
        <span className="row__title"><span className="row__text">{title}</span>{badge}</span>
        {sub && <span className="row__sub">{sub}</span>}
      </span>
      {(amount !== undefined || end !== undefined) && (
        <span className="row__end">
          {/* Le montant et ce qui suit : un bouton « Supprimer » donné avec le montant restait
              caché, sans un mot (essais C7 et C9 de Claw Web, 30 octobre 2026). */}
          {amount !== undefined && <span className={`row__amount${amount > 0 ? " row__amount--in" : ""}`}>{signed(amount)}</span>}
          {end}
          {endNote && <small>{endNote}</small>}
        </span>
      )}
      {action && (
        <button className="icon-btn row__action" type="button" aria-label={action.label} onClick={action.onClick}>
          <Icon icon={action.icon} />
        </button>
      )}
    </motion.li>
  );
}

/** Un statut court (« Reçu », « En attente »). */
export function Badge({ tone = "accent", children }: { tone?: Tone; children: ReactNode }) {
  return <span className={`badge badge--${tone}`}>{children}</span>;
}

/** Des jauges les unes sous les autres (des <Meter>). */
export function Meters({ children }: { children: ReactNode }) {
  return <div className="meters">{children}</div>;
}

/** Une jauge : `value` de 0 à 1 ; `valueText` à droite du nom (« 120 € sur 300 € »). */
export function Meter({ name, value, valueText, note, tone }: { name: ReactNode; value: number; valueText?: ReactNode; note?: ReactNode; tone?: "in" | "out" }) {
  const percent = Math.round(Math.min(1, Math.max(0, value || 0)) * 100);
  return (
    <div className="meter">
      <div className="meter__head">
        <span className="meter__name">{name}</span>
        {valueText && <span className="meter__value">{valueText}</span>}
      </div>
      <div className={`meter__track${tone ? ` meter__track--${tone}` : ""}`} role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent} aria-label={typeof name === "string" ? name : undefined}>
        <i style={{ "--value": `${percent}%` } as CSSProperties} />
      </div>
      {note && <p className="meter__note">{note}</p>}
    </div>
  );
}

/** Rien encore : une icône, une phrase, et l'action qui remplit (« Ajouter une opération »). */
export function Empty({ icon, text, action }: { icon?: LucideIcon; text: ReactNode; action?: { label: string; onClick: () => void } }) {
  return (
    <div className="empty">
      {icon && <Icon icon={icon} size={36} />}
      <p>{text}</p>
      {action && (
        <button className="btn btn--primary btn--small" type="button" onClick={action.onClick}>{action.label}</button>
      )}
    </div>
  );
}
