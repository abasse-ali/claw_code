// Des cartes (Leaflet, fonds d'OpenStreetMap) : une carte avec ses épingles et un trajet, la
// position du téléphone, la recherche d'une adresse, les distances. La carte n'est
// téléchargée qu'à son premier affichage. Les fonds de carte viennent d'internet : hors
// ligne, la carte reste grise et le dit.

import { LocateFixed, MapPin, Search } from "lucide-react";
import { lazy, Suspense, useEffect, useRef, useState, type FormEvent } from "react";
import { Icon } from "./icons";
import { useOnline } from "./plus";

export type Place = { lat: number; lng: number };
export type MapMarker = Place & { id: string; label: string; note?: string; tone?: "accent" | "in" | "out" | "warn" };
export type MapViewProps = {
  /** Le centre de la carte (Paris : { lat: 48.8566, lng: 2.3522 }). */
  center: Place;
  zoom?: number;
  markers?: MapMarker[];
  /** Un trajet : la suite de ses points. */
  route?: Place[];
  /** Un toucher sur la carte : l'endroit touché (poser une épingle). */
  onPick?: (place: Place) => void;
  /** Un toucher sur une épingle : son id. */
  onMarker?: (id: string) => void;
  /** Cadrer toutes les épingles et le trajet (par défaut). */
  fit?: boolean;
  height?: number;
  label: string;
};

const MapViewImpl = lazy(() => import("./lazy/map-view"));

/** Une carte avec ses épingles (markers), un trajet (route), et onPick pour poser un lieu. */
export function MapView(props: MapViewProps) {
  const online = useOnline();
  const height = props.height ?? 280;
  return (
    <div className="map">
      <Suspense fallback={<div className="map-view map-view--loading" style={{ height }}>Chargement de la carte…</div>}>
        <MapViewImpl {...props} />
      </Suspense>
      {!online && <p className="map__offline">Hors ligne : le fond de carte reviendra avec le réseau.</p>}
    </div>
  );
}

/** La position du téléphone, demandée au premier appel de locate() (le téléphone demande la
 * permission) ; `error` est une phrase claire quand c'est refusé ou impossible. */
export function useGeolocation() {
  const [state, setState] = useState<{ position: Place | null; accuracy: number | null; error: string; busy: boolean }>({ position: null, accuracy: null, error: "", busy: false });
  const locate = () => {
    if (!("geolocation" in navigator)) { setState((now) => ({ ...now, error: "Ce téléphone ne donne pas sa position." })); return; }
    setState((now) => ({ ...now, busy: true, error: "" }));
    navigator.geolocation.getCurrentPosition(
      (found) => setState({ position: { lat: found.coords.latitude, lng: found.coords.longitude }, accuracy: found.coords.accuracy, error: "", busy: false }),
      (failure) => setState((now) => ({ ...now, busy: false, error: failure.code === failure.PERMISSION_DENIED ? "La position est refusée : autorisez-la dans les réglages du navigateur." : "La position n'a pas pu être trouvée." })),
      { enableHighAccuracy: true, timeout: 15000, maximumAge: 60000 },
    );
  };
  return { ...state, locate };
}

/** Le bouton « Ma position », qui centre sur le téléphone (onLocate reçoit le lieu). */
export function LocateButton({ onLocate, label = "Ma position" }: { onLocate: (place: Place) => void; label?: string }) {
  const { error, busy, locate, position } = useGeolocation();
  const [asked, setAsked] = useState(false);
  const handler = useRef(onLocate);
  handler.current = onLocate;
  useEffect(() => {
    if (asked && position) { setAsked(false); handler.current(position); }
  }, [asked, position]);
  return (
    <span className="locate">
      <button className="btn btn--small" type="button" disabled={busy} onClick={() => { setAsked(true); locate(); }}><Icon icon={LocateFixed} size={18} />{busy ? "Recherche…" : label}</button>
      {error && <small className="locate__error" role="alert">{error}</small>}
    </span>
  );
}

/** La distance à vol d'oiseau entre deux lieux, en kilomètres. */
export function distanceKm(a: Place, b: Place): number {
  const rad = (value: number) => (value * Math.PI) / 180;
  const dLat = rad(b.lat - a.lat);
  const dLng = rad(b.lng - a.lng);
  const h = Math.sin(dLat / 2) ** 2 + Math.cos(rad(a.lat)) * Math.cos(rad(b.lat)) * Math.sin(dLng / 2) ** 2;
  return 6371 * 2 * Math.asin(Math.sqrt(h));
}

/** « 850 m », « 2,4 km », « 120 km ». */
export function formatDistance(km: number): string {
  if (km < 1) return `${Math.round(km * 1000)} m`;
  return `${new Intl.NumberFormat("fr-FR", { maximumFractionDigits: km < 10 ? 1 : 0 }).format(km)} km`;
}

/** Cherche une adresse ou un lieu (OpenStreetMap) : les 5 meilleurs résultats, en français. */
export async function searchPlace(query: string): Promise<(Place & { label: string })[]> {
  const url = `https://nominatim.openstreetmap.org/search?format=jsonv2&limit=5&accept-language=fr&q=${encodeURIComponent(query)}`;
  const response = await fetch(url, { headers: { Accept: "application/json" } });
  if (!response.ok) throw new Error("La recherche d'adresse ne répond pas.");
  const found = (await response.json()) as { lat: string; lon: string; display_name: string }[];
  return found.map((item) => ({ lat: Number(item.lat), lng: Number(item.lon), label: item.display_name }));
}

/** Un champ pour chercher une adresse, et la liste des lieux trouvés à toucher. */
export function PlaceSearch({ onPick, label = "Adresse", placeholder = "12 rue de la Paix, Paris" }: { onPick: (place: Place & { label: string }) => void; label?: string; placeholder?: string }) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<(Place & { label: string })[]>([]);
  const [state, setState] = useState<"" | "busy" | "none" | "error">("");
  const search = async (event: FormEvent) => {
    event.preventDefault();
    if (query.trim().length < 3) return;
    setState("busy");
    try {
      const found = await searchPlace(query.trim());
      setResults(found);
      setState(found.length ? "" : "none");
    } catch {
      setState("error");
    }
  };
  return (
    <div className="place-search">
      <form className="place-search__row" onSubmit={search} role="search">
        <label className="visually-hidden" htmlFor="place-search">{label}</label>
        <input id="place-search" className="place-search__input" placeholder={placeholder} value={query} onChange={(event) => setQuery(event.target.value)} autoComplete="street-address" />
        <button className="btn btn--primary" type="submit" disabled={state === "busy"} aria-label="Chercher"><Icon icon={Search} size={18} /></button>
      </form>
      {state === "none" && <p className="place-search__note">Aucun lieu trouvé.</p>}
      {state === "error" && <p className="place-search__note" role="alert">La recherche ne répond pas : vérifiez le réseau.</p>}
      {results.length > 0 && (
        <ul className="place-search__results">
          {results.map((place) => (
            <li key={`${place.lat},${place.lng}`}>
              <button type="button" onClick={() => { onPick(place); setResults([]); setQuery(place.label.split(",")[0]); }}>
                <Icon icon={MapPin} size={18} /><span>{place.label}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
