// Icônes de Lucide (lucide-react) : `<Icon icon={Wallet} />` les dessine à la taille du kit.
// Toutes les icônes : https://lucide.dev/icons (import { NomEnPascal } from "lucide-react").

import {
  Banknote,
  BookOpen,
  Briefcase,
  Bus,
  Car,
  Coffee,
  Dumbbell,
  Gamepad2,
  Gift,
  GraduationCap,
  HeartPulse,
  House,
  Landmark,
  PiggyBank,
  Plane,
  Repeat,
  Shield,
  ShoppingBag,
  ShoppingCart,
  Smartphone,
  Tag,
  Utensils,
  Zap,
  type LucideIcon,
} from "lucide-react";

export type { LucideIcon };

export function Icon({ icon: Glyph, size = 20 }: { icon: LucideIcon; size?: number }) {
  return <Glyph className="icon" size={size} aria-hidden="true" />;
}

const words: [LucideIcon, string][] = [
  [House, "logement loyer maison appartement rent home housing"],
  [ShoppingCart, "courses alimentaire alimentation nourriture supermarche epicerie groceries food"],
  [Bus, "transport transports bus metro tram train navigo velo"],
  [Car, "voiture essence carburant parking peage car fuel"],
  [Smartphone, "forfait telephone mobile internet box phone"],
  [Repeat, "abonnement abonnements streaming subscription netflix spotify"],
  [Utensils, "restaurant restos resto sorties repas cantine"],
  [Gamepad2, "loisirs jeux cinema sortie games leisure"],
  [HeartPulse, "sante pharmacie medecin mutuelle health"],
  [Briefcase, "salaire revenus revenu travail paie prime salary income job"],
  [Landmark, "caf aides aide allocation allocations bourse apl banque"],
  [Banknote, "especes espece cash liquide"],
  [PiggyBank, "epargne economies livret savings"],
  [ShoppingBag, "shopping vetements habits clothes"],
  [Gift, "cadeaux cadeau gift"],
  [Zap, "energie electricite gaz eau energy"],
  [Shield, "assurance assurances insurance"],
  [GraduationCap, "etudes ecole education formation"],
  [Coffee, "cafe boulangerie snack"],
  [Dumbbell, "sport salle gym"],
  [Plane, "voyage vacances avion travel"],
  [BookOpen, "livres livre book"],
];

const plain = (text: string) =>
  String(text || "").toLowerCase().normalize("NFD").replace(/[̀-ͯ]/g, "");

/** L'icône qui va avec une catégorie ou un libellé (« Courses » : un caddie), sinon une étiquette. */
export function iconFor(...names: string[]): LucideIcon {
  for (const name of names) {
    const found = plain(name).split(/[^a-z0-9-]+/).filter(Boolean);
    const match = words.find(([, list]) => found.some((word) => list.split(" ").includes(word)));
    if (match) return match[0];
  }
  return Tag;
}
