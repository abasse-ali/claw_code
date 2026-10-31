//! Les calculs de l'appli, en Rust, appelés depuis React :
//!
//! ```ts
//! const { exports } = useWasm<{ loan_payment(capital: number, rate: number, months: number): number }>();
//! const monthly = exports?.loan_payment(15000, 0.045, 60);
//! ```
//!
//! Chaque fonction `#[no_mangle] pub extern "C"` est visible de l'appli. Elles prennent et
//! rendent des nombres (f64, i32, u32) : un texte ou une liste passe par la mémoire du module,
//! le plus simple est de garder les textes côté React. `cargo test` vérifie les calculs.

/// La mensualité d'un prêt : `capital` emprunté, `annual_rate` (0.045 pour 4,5 %), sur `months`.
#[no_mangle]
pub extern "C" fn loan_payment(capital: f64, annual_rate: f64, months: u32) -> f64 {
    if months == 0 {
        return 0.0;
    }
    let rate = annual_rate / 12.0;
    if rate.abs() < 1e-12 {
        return capital / f64::from(months);
    }
    capital * rate / (1.0 - (1.0 + rate).powi(-(months as i32)))
}

/// Ce que devient une épargne de `monthly` par mois pendant `months`, placée à `annual_rate`.
#[no_mangle]
pub extern "C" fn savings_after(monthly: f64, annual_rate: f64, months: u32) -> f64 {
    let rate = annual_rate / 12.0;
    (0..months).fold(0.0, |total, _| total * (1.0 + rate) + monthly)
}

/// Le nombre de mois pour atteindre `target` en mettant `monthly` de côté (0 : jamais).
#[no_mangle]
pub extern "C" fn months_to_goal(target: f64, saved: f64, monthly: f64) -> u32 {
    if saved >= target {
        return 0;
    }
    if monthly <= 0.0 {
        return 0;
    }
    ((target - saved) / monthly).ceil() as u32
}

/// Le nombre de nombres premiers jusqu'à `limit` : un calcul lourd, rapide en WebAssembly.
#[no_mangle]
pub extern "C" fn count_primes(limit: u32) -> u32 {
    if limit < 2 {
        return 0;
    }
    let size = limit as usize + 1;
    let mut composite = vec![false; size];
    let mut count = 0;
    for number in 2..size {
        if composite[number] {
            continue;
        }
        count += 1;
        let mut multiple = number * number;
        while multiple < size {
            composite[multiple] = true;
            multiple += number;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_loan_is_paid_back_in_equal_months() {
        let monthly = loan_payment(15_000.0, 0.045, 60);
        assert!((monthly - 279.65).abs() < 0.01, "{monthly}");
        assert!((loan_payment(1200.0, 0.0, 12) - 100.0).abs() < 1e-9);
        assert_eq!(loan_payment(1000.0, 0.05, 0), 0.0);
    }

    #[test]
    fn savings_grow_and_goals_are_reached() {
        assert!((savings_after(100.0, 0.0, 12) - 1200.0).abs() < 1e-9);
        assert!(savings_after(100.0, 0.03, 12) > 1200.0);
        assert_eq!(months_to_goal(1000.0, 200.0, 100.0), 8);
        assert_eq!(months_to_goal(1000.0, 1000.0, 100.0), 0);
    }

    #[test]
    fn primes_are_counted() {
        assert_eq!(count_primes(10), 4);
        assert_eq!(count_primes(100_000), 9592);
    }
}
