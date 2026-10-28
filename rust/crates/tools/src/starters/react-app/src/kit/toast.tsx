// Messages courts en bas de l'écran, un à la fois, avec une action au besoin :
// toast("Dépense enregistrée", { action: { label: "Annuler", run: undo } }).
// <Toaster /> est déjà posé par <AppShell>.

import { AnimatePresence, motion } from "framer-motion";
import { useEffect } from "react";
import { create } from "zustand";

type Toast = {
  id: number;
  message: string;
  tone?: "error";
  action?: { label?: string; run: () => void };
  duration?: number;
};

const useToasts = create<{ current: Toast | null }>(() => ({ current: null }));
let next = 0;

export function toast(message: string, options: Omit<Toast, "id" | "message"> = {}): void {
  useToasts.setState({ current: { id: next++, message, ...options } });
}

export function Toaster() {
  const current = useToasts((state) => state.current);
  useEffect(() => {
    if (!current) return;
    const timer = setTimeout(
      () => useToasts.setState((state) => (state.current?.id === current.id ? { current: null } : state)),
      current.duration ?? (current.action ? 6000 : 3500),
    );
    return () => clearTimeout(timer);
  }, [current]);
  return (
    <div className="toasts" aria-live="polite">
      <AnimatePresence>
        {current && (
          <motion.div
            key={current.id}
            className={`toast${current.tone === "error" ? " is-error" : ""}`}
            role={current.tone === "error" ? "alert" : "status"}
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 8 }}
            transition={{ duration: 0.2 }}
          >
            <span className="toast__text">{current.message}</span>
            {current.action && (
              <button
                type="button"
                className="toast__action"
                onClick={() => {
                  current.action?.run();
                  useToasts.setState({ current: null });
                }}
              >
                {current.action.label ?? "Annuler"}
              </button>
            )}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
