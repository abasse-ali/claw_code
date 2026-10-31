// Les blocs des applis de boutique et des réseaux : fiche produit, prix (barré en promotion),
// panier ; publication, commentaires, en-tête de profil, liste de notifications.

import { AnimatePresence, motion } from "framer-motion";
import { Heart, MessageCircle, Minus, Plus, Share2, ShoppingBag, Star, Trash2 } from "lucide-react";
import { useState, type FormEvent, type ReactNode } from "react";
import { Avatar } from "./blocks";
import { money } from "./format";
import { Icon } from "./icons";

const EASE = [0.2, 0.9, 0.3, 1] as const;

// --- Boutique -------------------------------------------------------------------------------

/** Un prix, et l'ancien barré avec le pourcentage de remise quand il y a une promotion. */
export function PriceTag({ price, was, size = "md" }: { price: number; was?: number; size?: "md" | "lg" }) {
  const off = was && was > price ? Math.round((1 - price / was) * 100) : 0;
  return (
    <span className={`price price--${size}`}>
      <b className={off ? "price__now is-promo" : "price__now"}>{money(price)}</b>
      {off > 0 && <><s className="price__was" aria-label={`au lieu de ${money(was!)}`}>{money(was!)}</s><span className="price__off">−{off} %</span></>}
    </span>
  );
}

/** Une fiche produit : photo, nom, note, prix, et « Ajouter » (ou − et + une fois au panier). */
export function ProductCard({ name, image, price, was, rating, note, quantity = 0, onChange }: {
  name: string; image?: string; price: number; was?: number; rating?: number; note?: string; quantity?: number; onChange: (quantity: number) => void;
}) {
  return (
    <article className="product">
      <div className="product__photo">{image ? <img src={image} alt="" loading="lazy" /> : <Icon icon={ShoppingBag} size={32} />}</div>
      <div className="product__body">
        <h3 className="product__name">{name}</h3>
        {note && <small className="product__note">{note}</small>}
        {rating !== undefined && <span className="product__rating" aria-label={`Noté ${rating.toFixed(1).replace(".", ",")} sur 5`}><Icon icon={Star} size={14} />{rating.toFixed(1).replace(".", ",")}</span>}
        <div className="product__foot">
          <PriceTag price={price} was={was} />
          {quantity > 0 ? (
            <span className="product__qty">
              <button className="icon-btn" type="button" aria-label={`Un ${name} de moins`} onClick={() => onChange(quantity - 1)}><Icon icon={quantity === 1 ? Trash2 : Minus} size={18} /></button>
              <motion.output key={quantity} initial={{ scale: 1.3 }} animate={{ scale: 1 }} aria-label="Quantité">{quantity}</motion.output>
              <button className="icon-btn" type="button" aria-label={`Un ${name} de plus`} onClick={() => onChange(quantity + 1)}><Icon icon={Plus} size={18} /></button>
            </span>
          ) : (
            <button className="btn btn--primary btn--small" type="button" onClick={() => onChange(1)}>Ajouter</button>
          )}
        </div>
      </div>
    </article>
  );
}

export type CartLine = { id: string; name: string; price: number; quantity: number };

/** Le panier : ses lignes, le sous-total, la livraison (offerte au-delà de `freeFrom`) et
 * le total, avec le bouton pour payer. */
export function CartSummary({ lines, delivery = 0, freeFrom, onCheckout, checkout = "Commander" }: {
  lines: CartLine[]; delivery?: number; freeFrom?: number; onCheckout: () => void; checkout?: string;
}) {
  const subtotal = lines.reduce((sum, line) => sum + line.price * line.quantity, 0);
  const free = freeFrom !== undefined && subtotal >= freeFrom;
  const shipping = lines.length && !free ? delivery : 0;
  if (!lines.length) return <p className="cart__empty">Le panier est vide.</p>;
  return (
    <div className="cart">
      <ul className="cart__lines">
        {lines.map((line) => (
          <li key={line.id}><span>{line.quantity} × {line.name}</span><b>{money(line.price * line.quantity)}</b></li>
        ))}
      </ul>
      <dl className="cart__sums">
        <div><dt>Sous-total</dt><dd>{money(subtotal)}</dd></div>
        <div><dt>Livraison</dt><dd>{shipping ? money(shipping) : "offerte"}</dd></div>
        {freeFrom !== undefined && !free && <p className="cart__hint">Encore {money(freeFrom - subtotal)} pour la livraison offerte.</p>}
        <div className="cart__total"><dt>Total</dt><dd>{money(subtotal + shipping)}</dd></div>
      </dl>
      <button className="btn btn--primary btn--block" type="button" onClick={onCheckout}>{checkout} · {money(subtotal + shipping)}</button>
    </div>
  );
}

// --- Réseau ---------------------------------------------------------------------------------

/** Une publication : auteur, moment, texte, photo, et J'aime, Commenter, Partager. */
export function PostCard({ author, time, text, image, likes, liked, onLike, comments = 0, onComment, onShare }: {
  author: string; time: string; text: ReactNode; image?: string; likes: number; liked: boolean; onLike: (liked: boolean) => void;
  comments?: number; onComment?: () => void; onShare?: () => void;
}) {
  return (
    <article className="post">
      <header className="post__head"><Avatar name={author} /><span><b>{author}</b><small>{time}</small></span></header>
      <div className="post__text">{text}</div>
      {image && <img className="post__photo" src={image} alt="" loading="lazy" />}
      <footer className="post__actions">
        <button type="button" className={`post__action${liked ? " is-liked" : ""}`} aria-pressed={liked} onClick={() => onLike(!liked)}>
          <motion.span key={String(liked)} initial={liked ? { scale: 1.4 } : false} animate={{ scale: 1 }} style={{ display: "grid" }}><Icon icon={Heart} size={20} /></motion.span>{likes}
        </button>
        {onComment && <button type="button" className="post__action" onClick={onComment}><Icon icon={MessageCircle} size={20} />{comments}</button>}
        {onShare && <button type="button" className="post__action" aria-label="Partager" onClick={onShare}><Icon icon={Share2} size={20} /></button>}
      </footer>
    </article>
  );
}

export type PostComment = { id: string; author: string; text: string; time: string };

/** Les commentaires et le champ pour en écrire un (Entrée envoie). */
export function CommentList({ comments, onSend, placeholder = "Écrire un commentaire…" }: { comments: PostComment[]; onSend: (text: string) => void; placeholder?: string }) {
  const [text, setText] = useState("");
  const send = (event: FormEvent) => {
    event.preventDefault();
    if (!text.trim()) return;
    onSend(text.trim());
    setText("");
  };
  return (
    <div className="comments">
      <ul className="comments__list">
        <AnimatePresence initial={false}>
          {comments.map((comment) => (
            <motion.li key={comment.id} initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0 }} transition={{ ease: EASE }}>
              <Avatar name={comment.author} size={32} />
              <span className="comments__bubble"><b>{comment.author}</b>{comment.text}<small>{comment.time}</small></span>
            </motion.li>
          ))}
        </AnimatePresence>
        {!comments.length && <li className="comments__empty">Aucun commentaire pour l'instant.</li>}
      </ul>
      <form className="comments__form" onSubmit={send}>
        <input aria-label="Commentaire" placeholder={placeholder} value={text} onChange={(event) => setText(event.target.value)} />
        <button className="btn btn--primary btn--small" type="submit" disabled={!text.trim()}>Envoyer</button>
      </form>
    </div>
  );
}

/** L'en-tête d'un profil : bandeau, avatar, nom, phrase, chiffres (abonnés…) et un bouton. */
export function ProfileHeader({ name, bio, stats = [], action }: { name: string; bio?: ReactNode; stats?: { label: string; value: ReactNode }[]; action?: ReactNode }) {
  return (
    <section className="profile">
      <div className="profile__cover" aria-hidden="true" />
      <div className="profile__main">
        <span className="profile__avatar"><Avatar name={name} size={84} /></span>
        <h2 className="profile__name">{name}</h2>
        {bio && <p className="profile__bio">{bio}</p>}
        {stats.length > 0 && (
          <dl className="profile__stats">
            {stats.map((stat) => <div key={stat.label}><dd>{stat.value}</dd><dt>{stat.label}</dt></div>)}
          </dl>
        )}
        {action && <div className="profile__action">{action}</div>}
      </div>
    </section>
  );
}

/** Suivre / Abonné, qui change d'allure une fois suivi. */
export function FollowButton({ following, onChange, follow = "Suivre", followed = "Abonné" }: { following: boolean; onChange: (following: boolean) => void; follow?: string; followed?: string }) {
  return (
    <button type="button" className={`btn btn--small ${following ? "" : "btn--primary"} follow`} aria-pressed={following} onClick={() => onChange(!following)}>
      {following ? followed : follow}
    </button>
  );
}

export type Notice = { id: string; title: string; text?: string; time: string; read?: boolean; from?: string };

/** La liste des notifications : les non lues en avant avec leur point, « Tout marquer comme
 * lu », et un toucher sur une notification la marque lue. */
export function NotificationList({ items, onRead, onReadAll }: { items: Notice[]; onRead: (id: string) => void; onReadAll: () => void }) {
  const unread = items.filter((item) => !item.read).length;
  return (
    <div className="notices">
      <div className="notices__head">
        <span>{unread ? `${unread} non lue${unread > 1 ? "s" : ""}` : "Tout est lu"}</span>
        {unread > 0 && <button className="btn btn--small" type="button" onClick={onReadAll}>Tout marquer comme lu</button>}
      </div>
      <ul className="notices__list">
        {items.map((item) => (
          <li key={item.id}>
            <button type="button" className={`notice${item.read ? "" : " is-unread"}`} onClick={() => onRead(item.id)}>
              {item.from && <Avatar name={item.from} size={36} />}
              <span className="notice__text"><b>{item.title}</b>{item.text && <span>{item.text}</span>}<small>{item.time}</small></span>
              {!item.read && <i className="notice__dot" aria-label="non lue" />}
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
