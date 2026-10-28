// Des réglages gardés sur le téléphone (les chiffres que l'utilisateur ajuste : budget,
// salaire, objectif). Exemple :
//   export const useBudget = persistentStore("budget", { monthly: 900, rent: 450 });
//   const monthly = useBudget((state) => state.monthly);
//   useBudget.setState({ monthly: 950 });

import { create } from "zustand";
import { persist } from "zustand/middleware";

export function persistentStore<T extends object>(name: string, initial: T) {
  return create<T>()(persist(() => initial, { name: `appli-${name}` }));
}
